#include "lld/Common/Driver.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/ADT/StringExtras.h"
#include "llvm/IR/Constants.h"
#include "llvm/IR/Function.h"
#include "llvm/IR/GlobalVariable.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/Intrinsics.h"
#include "llvm/IR/IntrinsicsAMDGPU.h"
#include "llvm/IR/LegacyPassManager.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/Verifier.h"
#include "llvm/MC/TargetRegistry.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Support/CodeGen.h"
#include "llvm/Support/Alignment.h"
#include "llvm/Support/FileSystem.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/TargetSelect.h"
#include "llvm/Support/raw_ostream.h"
#include "llvm/Target/TargetMachine.h"
#include "llvm/TargetParser/Triple.h"

#include <hip/hip_runtime.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstring>
#include <ctime>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <unistd.h>
#include <vector>

LLD_HAS_DRIVER(elf)

using namespace llvm;

namespace {

constexpr uint32_t U32Mask = 0xffffffffu;
constexpr unsigned ThreadsPerBlock = 256;
constexpr unsigned InputCount = 256;

constexpr StringRef DataLayout =
    "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-"
    "p6:32:32-p7:160:256:256:32-p8:128:128-p9:192:256:256:32-"
    "i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-"
    "v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9";

struct ByteStream {
  const uint8_t *Data;
  size_t Size;
  size_t Pos = 0;

  uint8_t next8() {
    if (Size == 0)
      return 0;
    uint8_t V = Data[Pos % Size];
    ++Pos;
    return V;
  }

  uint32_t next32() {
    uint32_t V = 0;
    for (unsigned I = 0; I < 4; ++I)
      V |= static_cast<uint32_t>(next8()) << (I * 8);
    return V;
  }

  uint64_t next64() {
    uint64_t V = 0;
    for (unsigned I = 0; I < 8; ++I)
      V |= static_cast<uint64_t>(next8()) << (I * 8);
    return V;
  }
};

struct Op {
  uint8_t Kind;
  uint64_t A;
  uint64_t B;
  uint64_t C;
};

struct Program {
  std::vector<Op> Ops;
  bool UseStructuredCFG = false;
  uint8_t CFGPrefix = 0;
  uint8_t CFGThen = 0;
  uint8_t CFGElse = 0;
  uint64_t CFGPredicate = 0;
};

uint32_t u32(uint64_t V) { return static_cast<uint32_t>(V & U32Mask); }

bool envFlag(const char *Name, bool Default) {
  const char *Value = std::getenv(Name);
  if (!Value || !*Value)
    return Default;
  return std::strcmp(Value, "0") != 0 && std::strcmp(Value, "false") != 0 &&
         std::strcmp(Value, "False") != 0 && std::strcmp(Value, "no") != 0 &&
         std::strcmp(Value, "off") != 0;
}

unsigned narrowVariant(const Op &O) { return O.C % 7; }

unsigned narrowShift(const Op &O, unsigned Bits) {
  return static_cast<unsigned>(O.B) & (Bits - 1);
}

bool isIdentityNarrow(const Op &O, unsigned Bits) {
  if (!((Bits == 8 && O.Kind == 18) || (Bits == 16 && O.Kind == 19)))
    return false;
  uint64_t Mask = (1ull << Bits) - 1ull;
  switch (narrowVariant(O)) {
  case 0:
  case 1:
  case 3:
    return (O.A & Mask) == 0;
  case 2:
    return (O.A & Mask) == 1;
  case 4:
  case 5:
  case 6:
    return narrowShift(O, Bits) == 0;
  default:
    return false;
  }
}

bool triggersM002(const Op &O) { return isIdentityNarrow(O, 8); }

bool triggersM009(const Op &O) { return isIdentityNarrow(O, 16); }

bool triggersM011(const Op &O) {
  if (O.Kind != 27)
    return false;
  uint64_t Mask = 0xffu;
  switch (narrowVariant(O)) {
  case 0:
  case 1:
  case 3:
    return (O.A & Mask) == 0;
  case 2:
    return (O.A & Mask) == 1;
  case 4:
  case 5:
  case 6:
    return narrowShift(O, 8) == 0;
  default:
    return false;
  }
}

bool triggersM010(const Op &O) {
  if (O.Kind != 28)
    return false;
  uint64_t Mask = 0xffffu;
  switch (narrowVariant(O)) {
  case 0:
  case 1:
  case 3:
    return (O.A & Mask) == 0;
  case 2:
    return (O.A & Mask) == 1;
  case 4:
  case 5:
  case 6:
    return narrowShift(O, 16) == 0;
  default:
    return false;
  }
}

void breakIdentityNarrow(Op &O) {
  switch (narrowVariant(O)) {
  case 0:
  case 1:
  case 2:
  case 3:
    ++O.A;
    break;
  case 4:
  case 5:
  case 6:
    ++O.B;
    break;
  }
}

bool isI32Shl(const Op &O) { return O.Kind == 6; }

bool isI32Add(const Op &O) { return O.Kind == 0; }

bool isI32AddZero(const Op &O) { return isI32Add(O) && u32(O.A) == 0; }

bool isI32ShlZero(const Op &O) { return isI32Shl(O) && (O.A & 31u) == 0; }

bool hasShlAddPairs(ArrayRef<Op> Ops, unsigned RequiredPairs) {
  unsigned Pairs = 0;
  bool NeedAdd = false;
  for (const Op &O : Ops) {
    if (NeedAdd) {
      if (isI32Add(O)) {
        ++Pairs;
        NeedAdd = false;
        if (Pairs >= RequiredPairs)
          return true;
        continue;
      }
      Pairs = 0;
      NeedAdd = false;
    } else if (isI32AddZero(O)) {
      continue;
    }

    if (isI32Shl(O))
      NeedAdd = true;
    else if (!isI32AddZero(O))
      Pairs = 0;
  }
  return false;
}

bool hasFiveShlAddPairs(ArrayRef<Op> Ops) { return hasShlAddPairs(Ops, 5); }

bool hasFourShlAddPairsBeforeCtpop(ArrayRef<Op> Ops) {
  return !Ops.empty() && Ops.back().Kind == 11 &&
         hasShlAddPairs(Ops.drop_back(), 4);
}

bool hasAddShlLadder(ArrayRef<Op> Ops) {
  unsigned Pairs = 0;
  bool NeedShl = false;
  for (const Op &O : Ops) {
    if (isI32AddZero(O) || isI32ShlZero(O))
      continue;

    if (NeedShl) {
      if (isI32Shl(O)) {
        ++Pairs;
        NeedShl = false;
        if (Pairs >= 4)
          return true;
        continue;
      }
      Pairs = 0;
      NeedShl = false;
    }

    if (isI32Add(O))
      NeedShl = true;
    else
      Pairs = 0;
  }
  return false;
}

void chooseStructuredSlices(const Program &P, size_t &Prefix, size_t &ThenLen,
                            size_t &ElseLen, size_t &SuffixStart) {
  size_t Count = P.Ops.size();
  size_t MaxPrefix = Count - 3;
  Prefix = P.CFGPrefix % (MaxPrefix + 1);

  size_t Remaining = Count - Prefix;
  size_t BranchBudget = Remaining - 1;
  ThenLen = 1 + (P.CFGThen % (BranchBudget - 1));
  size_t ElseBudget = BranchBudget - ThenLen;
  ElseLen = 1 + (P.CFGElse % ElseBudget);
  SuffixStart = Prefix + ThenLen + ElseLen;
}

bool isVectorOp(const Op &O) { return O.Kind == 21 || O.Kind == 22; }

bool isPrivateMemoryOp(const Op &O) { return O.Kind == 53 || O.Kind == 58; }

bool isFshlOp(const Op &O) {
  return O.Kind == 29 || O.Kind == 42 || O.Kind == 54 || O.Kind == 58;
}

void breakFshl(Op &O) {
  switch (O.Kind) {
  case 29:
    O.Kind = 30;
    break;
  case 42:
  case 54:
    O.Kind = 55;
    break;
  case 58:
    O.Kind = 53;
    break;
  default:
    break;
  }
}

bool hasThreePrivateMemoryOps(ArrayRef<Op> Ops) {
  unsigned Count = 0;
  for (const Op &O : Ops) {
    if (isPrivateMemoryOp(O) && ++Count >= 3)
      return true;
  }
  return false;
}

bool hasTwoPrivateMemoryOps(ArrayRef<Op> Ops) {
  unsigned Count = 0;
  for (const Op &O : Ops) {
    if (isPrivateMemoryOp(O) && ++Count >= 2)
      return true;
  }
  return false;
}

bool isIdentityVectorLane0(const Op &O) {
  if (!isVectorOp(O))
    return false;
  unsigned Lanes = O.Kind == 21 ? 2 : 4;
  if ((O.B % Lanes) != 0)
    return false;

  uint32_t C0 = u32(O.A >> 32);
  switch ((O.A ^ O.C) % 8) {
  case 0:
  case 1:
  case 3:
  case 5:
    return C0 == 0;
  case 2:
    return C0 == 1;
  case 4:
    return C0 == U32Mask;
  case 6:
    return (C0 & 31u) == 0;
  default:
    return false;
  }
}

bool isVectorLane0AndXor(const Op &O) {
  if (!isVectorOp(O))
    return false;
  unsigned Lanes = O.Kind == 21 ? 2 : 4;
  if ((O.B % Lanes) != 0)
    return false;
  if (((O.A ^ O.C) % 8) != 4)
    return false;

  uint32_t C0 = u32(O.A >> 32);
  return C0 != 0 && C0 != U32Mask;
}

Program makeProgram(const uint8_t *Data, size_t Size) {
  ByteStream BS{Data, Size};
  Program P;
  unsigned OpCount = 1 + (BS.next8() % 48);
  bool AllowM001 = envFlag("FUZZX_ALLOW_M001_ASHR_I16_ZEXT", false);
  bool AllowM002 = envFlag("FUZZX_ALLOW_M002_I8_CLEAR_XOR", false) ||
                   envFlag("FUZZX_ALLOW_M006_I8_CLEAR_XOR", false) ||
                   envFlag("FUZZX_ALLOW_M008_I8_CLEAR_XOR", false);
  bool AllowM009 = envFlag("FUZZX_ALLOW_M009_I16_CLEAR_XOR", false);
  bool AllowM010 = envFlag("FUZZX_ALLOW_M010_I16_SEXT_CLEAR_XOR", false);
  bool AllowM011 = envFlag("FUZZX_ALLOW_M011_I8_SEXT_CLEAR_XOR", false);
  bool AllowM003 = envFlag("FUZZX_ALLOW_M003_SHL3_ADD_CHAIN", false) ||
                   envFlag("FUZZX_ALLOW_M005_SHL_ADD_CHAIN", false) ||
                   envFlag("FUZZX_ALLOW_M012_ADD_SHL_LADDER", false);
  bool AllowM014 = envFlag("FUZZX_ALLOW_M014_SHL_ADD_CTPOP", false);
  bool AllowM004 = envFlag("FUZZX_ALLOW_M004_VECTOR_IDENTITY_XOR", false) ||
                   envFlag("FUZZX_ALLOW_M007_VECTOR_IDENTITY_XOR", false);
  bool AllowM017 =
      envFlag("FUZZX_ALLOW_M017_VECTOR_AND_LANE0_CLEAR_XOR", false);
  bool AllowM013 = envFlag("FUZZX_ALLOW_M013_PRIVATE_MEMORY_FSHL", false);
  bool AllowM018 =
      envFlag("FUZZX_ALLOW_M018_TWO_PRIVATE_MEMORY_OPS", false);
  bool AllowM016 = envFlag("FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO", false) ||
                   envFlag("FUZZX_ALLOW_M016_SCALAR_FSHL", false);
  P.Ops.reserve(OpCount);
  for (unsigned I = 0; I < OpCount; ++I) {
    uint8_t RawKind = BS.next8();
    uint8_t Kind = RawKind % 27;
    if ((RawKind & 0xf0u) == 0x80u)
      Kind = 135 + ((RawKind & 15u) % 8);
    if ((RawKind & 0xf0u) == 0x50u)
      Kind = 167 + ((RawKind & 15u) % 6);
    if ((RawKind & 0xf0u) == 0x40u)
      Kind = 173 + ((RawKind & 15u) % 6);
    if ((RawKind & 0xf0u) == 0x30u)
      Kind = 179 + ((RawKind & 15u) % 6);
    if ((RawKind & 0xf0u) == 0x20u)
      Kind = 195 + ((RawKind & 15u) % 10);
    if ((RawKind & 0xf0u) == 0x10u)
      Kind = 185 + ((RawKind & 15u) % 10);
    if ((RawKind & 0xf0u) == 0x60u)
      Kind = 159 + ((RawKind & 15u) % 8);
    if ((RawKind & 0xf0u) == 0x70u)
      Kind = 143 + (RawKind & 15u);
    if ((RawKind & 0xf0u) == 0x90u)
      Kind = 119 + (RawKind & 15u);
    if ((RawKind & 0xf0u) == 0xa0u)
      Kind = 103 + (RawKind & 15u);
    if ((RawKind & 0xf0u) == 0xb0u)
      Kind = 91 + (RawKind & 15u);
    if ((RawKind & 0xf0u) == 0xc0u)
      Kind = 75 + (RawKind & 15u);
    if ((RawKind & 0xf0u) == 0xd0u)
      Kind = 59 + (RawKind & 15u);
    if ((RawKind & 0xf0u) == 0xe0u)
      Kind = 43 + (RawKind & 15u);
    if ((RawKind & 0xf0u) == 0xf0u)
      Kind = 27 + (RawKind & 15u);
    Op O{Kind, BS.next64(), BS.next64(), BS.next64()};
    if (!AllowM001 && O.Kind == 19 && O.C % 7 == 6)
      ++O.C;
    if (!AllowM002 && triggersM002(O))
      breakIdentityNarrow(O);
    if (!AllowM009 && triggersM009(O))
      breakIdentityNarrow(O);
    if (!AllowM010 && triggersM010(O))
      breakIdentityNarrow(O);
    if (!AllowM011 && triggersM011(O))
      breakIdentityNarrow(O);
    if (!AllowM004 && isIdentityVectorLane0(O))
      ++O.B;
    if (!AllowM017 && isVectorLane0AndXor(O))
      ++O.B;
    if (!AllowM016 && isFshlOp(O))
      breakFshl(O);
    P.Ops.push_back(O);
    if (!AllowM003) {
      if (hasFiveShlAddPairs(P.Ops)) {
        P.Ops.back().Kind = 3;
        ++P.Ops.back().A;
      } else if (hasAddShlLadder(P.Ops)) {
        P.Ops.back().Kind = 11;
      }
    }
    if (!AllowM014 && hasFourShlAddPairsBeforeCtpop(P.Ops))
      P.Ops.back().Kind = 0;
    if (!AllowM018 && hasTwoPrivateMemoryOps(P.Ops))
      P.Ops.back().Kind = 55;
    if (!AllowM013 && hasThreePrivateMemoryOps(P.Ops))
      P.Ops.back().Kind = AllowM016 ? 54 : 55;
  }
  P.UseStructuredCFG = P.Ops.size() >= 4 && (BS.next8() & 1);
  P.CFGPrefix = BS.next8();
  P.CFGThen = BS.next8();
  P.CFGElse = BS.next8();
  P.CFGPredicate = BS.next64();
  return P;
}

std::array<uint32_t, InputCount> makeInputs(const uint8_t *Data, size_t Size) {
  std::array<uint32_t, InputCount> Inputs{};
  std::array<uint32_t, 8> Edges = {0,          1,          0x7fffffffu,
                                   0x80000000u, 0xffffffffu, 0x55555555u,
                                   0xaaaaaaaau, 0x01010101u};
  std::copy(Edges.begin(), Edges.end(), Inputs.begin());
  uint64_t State = 0xa5a55a5ad15ea5edull ^ Size;
  for (size_t I = 0; I < Size; ++I)
    State = State * 6364136223846793005ull + Data[I] + 1442695040888963407ull;
  for (unsigned I = Edges.size(); I < InputCount; ++I) {
    State = State * 2862933555777941757ull + 3037000493ull;
    Inputs[I] = static_cast<uint32_t>(State >> 16);
  }
  return Inputs;
}

bool allowM015ScalarFshlZero();

bool oracleEnabled() {
  static const bool Enabled = envFlag("FUZZX_ENABLE_ORACLE", false);
  return Enabled;
}

uint64_t maskBits(unsigned Bits) {
  return Bits == 64 ? ~0ull : ((1ull << Bits) - 1ull);
}

uint64_t truncBits(uint64_t V, unsigned Bits) { return V & maskBits(Bits); }

int64_t signedBits(uint64_t V, unsigned Bits) {
  V = truncBits(V, Bits);
  if (Bits == 64)
    return static_cast<int64_t>(V);
  uint64_t Sign = 1ull << (Bits - 1);
  return static_cast<int64_t>((V ^ Sign) - Sign);
}

uint32_t zextBits(uint64_t V, unsigned Bits) {
  return static_cast<uint32_t>(truncBits(V, Bits));
}

uint32_t sextBits(uint64_t V, unsigned Bits) {
  return static_cast<uint32_t>(signedBits(V, Bits));
}

bool slt32(uint32_t A, uint32_t B) {
  return static_cast<int32_t>(A) < static_cast<int32_t>(B);
}

uint64_t ashrBits(uint64_t V, unsigned Bits, unsigned Shift) {
  return truncBits(static_cast<uint64_t>(signedBits(V, Bits) >> Shift), Bits);
}

uint64_t ctlzBits(uint64_t V, unsigned Bits) {
  V = truncBits(V, Bits);
  if (V == 0)
    return Bits;
  unsigned Count = 0;
  for (int I = static_cast<int>(Bits) - 1; I >= 0; --I) {
    if ((V >> I) & 1ull)
      break;
    ++Count;
  }
  return Count;
}

uint64_t cttzBits(uint64_t V, unsigned Bits) {
  V = truncBits(V, Bits);
  if (V == 0)
    return Bits;
  unsigned Count = 0;
  while (((V >> Count) & 1ull) == 0)
    ++Count;
  return Count;
}

uint64_t ctpopBits(uint64_t V, unsigned Bits) {
  V = truncBits(V, Bits);
  unsigned Count = 0;
  for (unsigned I = 0; I < Bits; ++I)
    Count += (V >> I) & 1ull;
  return Count;
}

uint64_t bitreverseBits(uint64_t V, unsigned Bits) {
  V = truncBits(V, Bits);
  uint64_t R = 0;
  for (unsigned I = 0; I < Bits; ++I)
    R |= ((V >> I) & 1ull) << (Bits - 1 - I);
  return R;
}

uint64_t bswapBits(uint64_t V, unsigned Bits) {
  V = truncBits(V, Bits);
  uint64_t R = 0;
  for (unsigned I = 0; I < Bits; I += 8)
    R |= ((V >> I) & 0xffull) << (Bits - 8 - I);
  return R;
}

uint64_t absBits(uint64_t V, unsigned Bits) {
  int64_t S = signedBits(V, Bits);
  if (S >= 0)
    return truncBits(V, Bits);
  if (Bits < 64 && S == -(1ll << (Bits - 1)))
    return truncBits(V, Bits);
  return truncBits(static_cast<uint64_t>(-S), Bits);
}

uint64_t uminBits(uint64_t A, uint64_t B, unsigned Bits) {
  A = truncBits(A, Bits);
  B = truncBits(B, Bits);
  return A < B ? A : B;
}

uint64_t umaxBits(uint64_t A, uint64_t B, unsigned Bits) {
  A = truncBits(A, Bits);
  B = truncBits(B, Bits);
  return A > B ? A : B;
}

uint64_t sminBits(uint64_t A, uint64_t B, unsigned Bits) {
  return signedBits(A, Bits) < signedBits(B, Bits) ? truncBits(A, Bits)
                                                   : truncBits(B, Bits);
}

uint64_t smaxBits(uint64_t A, uint64_t B, unsigned Bits) {
  return signedBits(A, Bits) > signedBits(B, Bits) ? truncBits(A, Bits)
                                                   : truncBits(B, Bits);
}

uint64_t fshlBits(uint64_t A, uint64_t B, uint64_t Shift, unsigned Bits) {
  A = truncBits(A, Bits);
  B = truncBits(B, Bits);
  Shift %= Bits;
  if (Shift == 0)
    return A;
  return truncBits((A << Shift) | (B >> (Bits - Shift)), Bits);
}

uint64_t fshrBits(uint64_t A, uint64_t B, uint64_t Shift, unsigned Bits) {
  A = truncBits(A, Bits);
  B = truncBits(B, Bits);
  Shift %= Bits;
  if (Shift == 0)
    return B;
  return truncBits((A << (Bits - Shift)) | (B >> Shift), Bits);
}

uint32_t adjustFshlShift(uint32_t Shift) {
  if (allowM015ScalarFshlZero())
    return Shift;
  return Shift == 0 ? 1 : Shift;
}

uint64_t uaddSat(uint64_t A, uint64_t B, unsigned Bits) {
  uint64_t Mask = maskBits(Bits);
  A &= Mask;
  B &= Mask;
  uint64_t R = A + B;
  return (R > Mask || R < A) ? Mask : R;
}

uint64_t usubSat(uint64_t A, uint64_t B, unsigned Bits) {
  A = truncBits(A, Bits);
  B = truncBits(B, Bits);
  return A < B ? 0 : A - B;
}

uint64_t saddSat(uint64_t A, uint64_t B, unsigned Bits) {
  int64_t SA = signedBits(A, Bits);
  int64_t SB = signedBits(B, Bits);
  int64_t Min = Bits == 64 ? INT64_MIN : -(1ll << (Bits - 1));
  int64_t Max = Bits == 64 ? INT64_MAX : ((1ll << (Bits - 1)) - 1);
  __int128 R = static_cast<__int128>(SA) + static_cast<__int128>(SB);
  if (R > Max)
    return truncBits(static_cast<uint64_t>(Max), Bits);
  if (R < Min)
    return truncBits(static_cast<uint64_t>(Min), Bits);
  return truncBits(static_cast<uint64_t>(static_cast<int64_t>(R)), Bits);
}

uint64_t ssubSat(uint64_t A, uint64_t B, unsigned Bits) {
  int64_t SA = signedBits(A, Bits);
  int64_t SB = signedBits(B, Bits);
  int64_t Min = Bits == 64 ? INT64_MIN : -(1ll << (Bits - 1));
  int64_t Max = Bits == 64 ? INT64_MAX : ((1ll << (Bits - 1)) - 1);
  __int128 R = static_cast<__int128>(SA) - static_cast<__int128>(SB);
  if (R > Max)
    return truncBits(static_cast<uint64_t>(Max), Bits);
  if (R < Min)
    return truncBits(static_cast<uint64_t>(Min), Bits);
  return truncBits(static_cast<uint64_t>(static_cast<int64_t>(R)), Bits);
}

struct OverflowResult {
  uint64_t Wrapped;
  bool Overflow;
};

OverflowResult uaddOverflow(uint64_t A, uint64_t B, unsigned Bits) {
  unsigned __int128 Sum = static_cast<unsigned __int128>(truncBits(A, Bits)) +
                          static_cast<unsigned __int128>(truncBits(B, Bits));
  return {truncBits(static_cast<uint64_t>(Sum), Bits), (Sum >> Bits) != 0};
}

OverflowResult usubOverflow(uint64_t A, uint64_t B, unsigned Bits) {
  A = truncBits(A, Bits);
  B = truncBits(B, Bits);
  return {truncBits(A - B, Bits), A < B};
}

OverflowResult umulOverflow(uint64_t A, uint64_t B, unsigned Bits) {
  unsigned __int128 Product =
      static_cast<unsigned __int128>(truncBits(A, Bits)) *
      static_cast<unsigned __int128>(truncBits(B, Bits));
  return {truncBits(static_cast<uint64_t>(Product), Bits),
          (Product >> Bits) != 0};
}

OverflowResult saddOverflow(uint64_t A, uint64_t B, unsigned Bits) {
  int64_t SA = signedBits(A, Bits);
  int64_t SB = signedBits(B, Bits);
  __int128 Sum = static_cast<__int128>(SA) + static_cast<__int128>(SB);
  __int128 Min = Bits == 64 ? static_cast<__int128>(INT64_MIN)
                            : -((__int128)1 << (Bits - 1));
  __int128 Max = Bits == 64 ? static_cast<__int128>(INT64_MAX)
                            : (((__int128)1 << (Bits - 1)) - 1);
  return {truncBits(static_cast<uint64_t>(static_cast<int64_t>(Sum)), Bits),
          Sum < Min || Sum > Max};
}

OverflowResult ssubOverflow(uint64_t A, uint64_t B, unsigned Bits) {
  int64_t SA = signedBits(A, Bits);
  int64_t SB = signedBits(B, Bits);
  __int128 Diff = static_cast<__int128>(SA) - static_cast<__int128>(SB);
  __int128 Min = Bits == 64 ? static_cast<__int128>(INT64_MIN)
                            : -((__int128)1 << (Bits - 1));
  __int128 Max = Bits == 64 ? static_cast<__int128>(INT64_MAX)
                            : (((__int128)1 << (Bits - 1)) - 1);
  return {truncBits(static_cast<uint64_t>(static_cast<int64_t>(Diff)), Bits),
          Diff < Min || Diff > Max};
}

OverflowResult smulOverflow(uint64_t A, uint64_t B, unsigned Bits) {
  int64_t SA = signedBits(A, Bits);
  int64_t SB = signedBits(B, Bits);
  __int128 Product = static_cast<__int128>(SA) * static_cast<__int128>(SB);
  __int128 Min = Bits == 64 ? static_cast<__int128>(INT64_MIN)
                            : -((__int128)1 << (Bits - 1));
  __int128 Max = Bits == 64 ? static_cast<__int128>(INT64_MAX)
                            : (((__int128)1 << (Bits - 1)) - 1);
  return {truncBits(static_cast<uint64_t>(static_cast<int64_t>(Product)), Bits),
          Product < Min || Product > Max};
}

uint32_t evalNarrow(uint32_t V, unsigned Bits, const Op &O,
                    bool SignedExtend = false) {
  uint64_t Mask = maskBits(Bits);
  uint64_t N = truncBits(V, Bits);
  unsigned Shift = static_cast<unsigned>(O.B) & (Bits - 1);
  uint64_t Mixed;
  switch (O.C % 7) {
  case 0:
    Mixed = N + (O.A & Mask);
    break;
  case 1:
    Mixed = N - (O.A & Mask);
    break;
  case 2:
    Mixed = N * (O.A & Mask);
    break;
  case 3:
    Mixed = N ^ (O.A & Mask);
    break;
  case 4:
    Mixed = N << Shift;
    break;
  case 5:
    Mixed = N >> Shift;
    break;
  default:
    Mixed = ashrBits(N, Bits, Shift);
    break;
  }
  uint32_t Extended =
      SignedExtend ? sextBits(Mixed, Bits) : zextBits(Mixed, Bits);
  return V ^ Extended;
}

uint32_t evalWide(uint32_t V, const Op &O) {
  uint64_t W = V;
  unsigned Shift = static_cast<unsigned>(O.B) & 63u;
  uint64_t Mixed;
  switch (O.C % 13) {
  case 0:
    Mixed = W + O.A;
    break;
  case 1:
    Mixed = W - O.A;
    break;
  case 2:
    Mixed = W * O.A;
    break;
  case 3:
    Mixed = W ^ O.A;
    break;
  case 4:
    Mixed = W & O.A;
    break;
  case 5:
    Mixed = W | O.A;
    break;
  case 6:
    Mixed = W << Shift;
    break;
  case 7:
    Mixed = W >> Shift;
    break;
  case 8:
    Mixed = ashrBits(W, 64, Shift);
    break;
  case 9:
    Mixed = ctlzBits(W, 64);
    break;
  case 10:
    Mixed = cttzBits(W, 64);
    break;
  case 11:
    Mixed = ctpopBits(W, 64);
    break;
  default:
    Mixed = bswapBits(W, 64);
    break;
  }
  return V + static_cast<uint32_t>(Mixed);
}

uint32_t evalOverflow(uint32_t V, const Op &O, unsigned Which) {
  OverflowResult R;
  switch (Which) {
  case 0:
    R = uaddOverflow(V, u32(O.A), 32);
    break;
  case 1:
    R = usubOverflow(V, u32(O.A), 32);
    break;
  case 2:
    R = saddOverflow(V, u32(O.A), 32);
    break;
  case 3:
    R = ssubOverflow(V, u32(O.A), 32);
    break;
  case 4:
    R = umulOverflow(V, u32(O.A), 32);
    break;
  default:
    R = smulOverflow(V, u32(O.A), 32);
    break;
  }
  return static_cast<uint32_t>(R.Wrapped) ^
         (static_cast<uint32_t>(R.Overflow) << (O.B & 31u));
}

uint32_t evalWideOverflow(uint32_t V, const Op &O, unsigned Which) {
  uint64_t W = V;
  OverflowResult R;
  switch (Which) {
  case 0:
    R = uaddOverflow(W, O.A, 64);
    break;
  case 1:
    R = usubOverflow(W, O.A, 64);
    break;
  case 2:
    R = saddOverflow(W, O.A, 64);
    break;
  case 3:
    R = ssubOverflow(W, O.A, 64);
    break;
  default:
    R = umulOverflow(W, O.A, 64);
    break;
  }
  uint32_t Lo = static_cast<uint32_t>(R.Wrapped);
  uint32_t Hi = static_cast<uint32_t>(R.Wrapped >> 32);
  return (Lo + Hi) ^ (static_cast<uint32_t>(R.Overflow) << (O.B & 31u));
}

std::array<uint32_t, 4> makeI32VectorValue(uint32_t V, unsigned Lanes,
                                           const Op &O) {
  std::array<uint32_t, 4> Cur = {};
  std::array<uint32_t, 4> Init = {u32(O.A), u32(O.B), u32(O.C),
                                  u32(O.A + O.B + O.C)};
  for (unsigned I = 0; I < Lanes; ++I)
    Cur[I] = I == 0 ? V : Init[I - 1];
  return Cur;
}

std::array<uint32_t, 4> makeDerivedI32VectorValue(uint32_t V, unsigned Lanes,
                                                  const Op &O) {
  std::array<uint32_t, 4> Cur = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (I) {
    case 1:
      Cur[I] = V ^ u32(O.A);
      break;
    case 2:
      Cur[I] = V + u32(O.B);
      break;
    default:
      Cur[I] = (V >> (O.C & 31u)) | u32(O.A >> 32);
      break;
    }
  }
  return Cur;
}

uint32_t evalVector(uint32_t V, unsigned Lanes, const Op &O) {
  std::array<uint32_t, 4> Cur = {};
  std::array<uint32_t, 4> Init = {u32(O.A), u32(O.B), u32(O.C),
                                  u32(O.A + O.B)};
  for (unsigned I = 0; I < Lanes; ++I)
    Cur[I] = I == 0 ? V : Init[I - 1];
  std::array<uint32_t, 4> C = {u32(O.A >> 32), u32(O.B >> 32),
                               u32(O.C >> 32), u32(O.A ^ O.C)};
  std::array<uint32_t, 4> Alts = {u32(~O.A), u32(~O.B), u32(~O.C),
                                  u32(O.A + O.B + O.C)};
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch ((O.A ^ O.C) % 8) {
    case 0:
      Mixed[I] = Cur[I] + C[I];
      break;
    case 1:
      Mixed[I] = Cur[I] - C[I];
      break;
    case 2:
      Mixed[I] = Cur[I] * C[I];
      break;
    case 3:
      Mixed[I] = Cur[I] ^ C[I];
      break;
    case 4:
      Mixed[I] = Cur[I] & C[I];
      break;
    case 5:
      Mixed[I] = Cur[I] | C[I];
      break;
    case 6:
      Mixed[I] = Cur[I] << (C[I] & 31u);
      break;
    default:
      Mixed[I] = Cur[I] < C[I] ? (Cur[I] ^ Alts[I]) : Cur[I];
      break;
    }
  }
  return V ^ Mixed[O.B % Lanes];
}

uint32_t evalI32VectorMinMax(uint32_t V, unsigned Lanes, const Op &O,
                             unsigned Which) {
  auto Cur = makeI32VectorValue(V, Lanes, O);
  std::array<uint32_t, 4> C = {u32(O.A >> 32), u32(O.B >> 32),
                               u32(O.C >> 32), u32(O.A ^ O.C)};
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (Which) {
    case 0:
      Mixed[I] = static_cast<uint32_t>(uminBits(Cur[I], C[I], 32));
      break;
    case 1:
      Mixed[I] = static_cast<uint32_t>(umaxBits(Cur[I], C[I], 32));
      break;
    case 2:
      Mixed[I] = static_cast<uint32_t>(sminBits(Cur[I], C[I], 32));
      break;
    default:
      Mixed[I] = static_cast<uint32_t>(smaxBits(Cur[I], C[I], 32));
      break;
    }
  }
  uint32_t Extracted = Mixed[O.B % Lanes];
  return Extracted + (V ^ (u32(O.C) | 1u));
}

uint32_t evalI32VectorBitIntrinsic(uint32_t V, unsigned Lanes, const Op &O,
                                   unsigned Which) {
  auto Cur = makeI32VectorValue(V, Lanes, O);
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (Which) {
    case 0:
      Mixed[I] = static_cast<uint32_t>(ctlzBits(Cur[I], 32));
      break;
    case 1:
      Mixed[I] = static_cast<uint32_t>(cttzBits(Cur[I], 32));
      break;
    default:
      Mixed[I] = static_cast<uint32_t>(ctpopBits(Cur[I], 32));
      break;
    }
  }
  uint32_t Extracted = Mixed[O.B % Lanes];
  return Extracted ^ (V + (u32(O.A) | 1u));
}

uint32_t evalI32VectorDynamicShift(uint32_t V, unsigned Lanes, const Op &O) {
  auto Cur = makeI32VectorValue(V, Lanes, O);
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    uint32_t Shift = Cur[I] & 31u;
    if (Shift == 0)
      Shift = 1;
    switch (O.C % 3) {
    case 0:
      Mixed[I] = Cur[I] << Shift;
      break;
    case 1:
      Mixed[I] = Cur[I] >> Shift;
      break;
    default:
      Mixed[I] = static_cast<uint32_t>(ashrBits(Cur[I], 32, Shift));
      break;
    }
  }
  return Mixed[O.B % Lanes] + u32(O.A);
}

uint32_t evalI32VectorFshr(uint32_t V, unsigned Lanes, const Op &O,
                           bool DynamicShift) {
  auto Cur = makeI32VectorValue(V, Lanes, O);
  std::array<uint32_t, 4> Other = {u32(O.A), u32(O.B), u32(O.C),
                                   u32(O.A ^ O.C)};
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    uint32_t Shift = DynamicShift ? (Cur[I] & 31u) : (Other[I] & 31u);
    Mixed[I] = static_cast<uint32_t>(fshrBits(Other[I], Cur[I], Shift, 32));
  }
  uint32_t Extracted = Mixed[O.B % Lanes];
  return Extracted ^ (V + (u32(O.C) | 1u));
}

uint32_t evalDerivedI32VectorMix(uint32_t V, unsigned Lanes, const Op &O) {
  auto Cur = makeDerivedI32VectorValue(V, Lanes, O);
  std::array<uint32_t, 4> C = {u32(O.A >> 32), u32(O.B >> 32),
                               u32(O.C >> 32), u32(O.A ^ O.C)};
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (O.C % 9) {
    case 0:
      Mixed[I] = Cur[I] + C[I];
      break;
    case 1:
      Mixed[I] = Cur[I] - C[I];
      break;
    case 2:
      Mixed[I] = Cur[I] * C[I];
      break;
    case 3:
      Mixed[I] = Cur[I] ^ C[I];
      break;
    case 4:
      Mixed[I] = Cur[I] & C[I];
      break;
    case 5:
      Mixed[I] = Cur[I] | C[I];
      break;
    case 6:
      Mixed[I] = Cur[I] << (C[I] & 31u);
      break;
    case 7:
      Mixed[I] = Cur[I] >> (C[I] & 31u);
      break;
    default: {
      uint32_t Shift = Cur[I] & 31u;
      if (Shift == 0)
        Shift = 1;
      Mixed[I] = static_cast<uint32_t>(ashrBits(Cur[I], 32, Shift));
      break;
    }
    }
  }
  uint32_t Extracted = Mixed[O.B % Lanes];
  return Extracted ^ (V + (u32(O.A) | 1u));
}

uint32_t evalDerivedI32VectorMinMax(uint32_t V, unsigned Lanes, const Op &O,
                                    unsigned Which) {
  auto Cur = makeDerivedI32VectorValue(V, Lanes, O);
  auto Other = makeDerivedI32VectorValue(V ^ u32(O.C), Lanes, O);
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (Which) {
    case 0:
      Mixed[I] = static_cast<uint32_t>(uminBits(Cur[I], Other[I], 32));
      break;
    case 1:
      Mixed[I] = static_cast<uint32_t>(umaxBits(Cur[I], Other[I], 32));
      break;
    case 2:
      Mixed[I] = static_cast<uint32_t>(sminBits(Cur[I], Other[I], 32));
      break;
    default:
      Mixed[I] = static_cast<uint32_t>(smaxBits(Cur[I], Other[I], 32));
      break;
    }
  }
  uint32_t Extracted = Mixed[O.B % Lanes];
  return Extracted + (V ^ (u32(O.B) | 1u));
}

uint32_t evalDerivedI32VectorUnary(uint32_t V, unsigned Lanes, const Op &O,
                                   unsigned Which) {
  auto Cur = makeDerivedI32VectorValue(V, Lanes, O);
  std::array<uint32_t, 4> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (Which) {
    case 0:
      Mixed[I] = static_cast<uint32_t>(bitreverseBits(Cur[I], 32));
      break;
    case 1:
      Mixed[I] = static_cast<uint32_t>(bswapBits(Cur[I], 32));
      break;
    default:
      Mixed[I] = static_cast<uint32_t>(absBits(Cur[I], 32));
      break;
    }
  }
  uint32_t Extracted = Mixed[O.B % Lanes];
  return Extracted ^ (V + (u32(O.C) | 1u));
}

std::array<uint64_t, 8> unpackLanes(uint32_t V, unsigned LaneBits) {
  std::array<uint64_t, 8> Lanes = {};
  uint64_t Mask = maskBits(LaneBits);
  for (unsigned I = 0; I < 32 / LaneBits; ++I)
    Lanes[I] = (V >> (I * LaneBits)) & Mask;
  return Lanes;
}

uint32_t packLanes(const std::array<uint64_t, 8> &Lanes, unsigned LaneBits) {
  uint32_t Packed = 0;
  uint64_t Mask = maskBits(LaneBits);
  for (unsigned I = 0; I < 32 / LaneBits; ++I)
    Packed |= static_cast<uint32_t>(Lanes[I] & Mask) << (I * LaneBits);
  return Packed;
}

std::array<uint64_t, 8> packedConstantLanes(unsigned LaneBits, uint64_t Bits,
                                            bool ShiftAmounts = false) {
  std::array<uint64_t, 8> Lanes = {};
  uint64_t Mask = maskBits(LaneBits);
  for (unsigned I = 0; I < 32 / LaneBits; ++I) {
    uint64_t Lane = (Bits >> (I * LaneBits)) & Mask;
    Lanes[I] = ShiftAmounts ? (Lane & (LaneBits - 1)) : Lane;
  }
  return Lanes;
}

uint32_t evalPackedVector(uint32_t V, unsigned LaneBits, const Op &O) {
  unsigned Lanes = 32 / LaneBits;
  auto Cur = unpackLanes(V, LaneBits);
  auto C0 = packedConstantLanes(LaneBits, O.A);
  auto C1 = packedConstantLanes(LaneBits, O.B);
  auto Shift = packedConstantLanes(LaneBits, O.B, true);
  std::array<uint64_t, 8> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (O.C % 11) {
    case 0:
      Mixed[I] = Cur[I] + C0[I];
      break;
    case 1:
      Mixed[I] = Cur[I] - C0[I];
      break;
    case 2:
      Mixed[I] = Cur[I] * C0[I];
      break;
    case 3:
      Mixed[I] = Cur[I] ^ C0[I];
      break;
    case 4:
      Mixed[I] = Cur[I] & C0[I];
      break;
    case 5:
      Mixed[I] = Cur[I] | C0[I];
      break;
    case 6:
      Mixed[I] = Cur[I] << Shift[I];
      break;
    case 7:
      Mixed[I] = Cur[I] >> Shift[I];
      break;
    case 8:
      Mixed[I] = ashrBits(Cur[I], LaneBits, Shift[I]);
      break;
    case 9:
      Mixed[I] = Cur[I] < C0[I] ? (Cur[I] ^ C1[I]) : (Cur[I] + C0[I]);
      break;
    default:
      Mixed[I] = signedBits(Cur[I], LaneBits) < signedBits(C0[I], LaneBits)
                     ? (Cur[I] - C1[I])
                     : (Cur[I] | C0[I]);
      break;
    }
  }
  return packLanes(Mixed, LaneBits) + (V ^ (u32(O.C) | 1u));
}

uint32_t evalPackedVectorSat(uint32_t V, unsigned LaneBits, const Op &O,
                             unsigned Which) {
  unsigned Lanes = 32 / LaneBits;
  auto Cur = unpackLanes(V, LaneBits);
  auto C = packedConstantLanes(LaneBits, O.A);
  std::array<uint64_t, 8> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (Which) {
    case 0:
      Mixed[I] = uaddSat(Cur[I], C[I], LaneBits);
      break;
    case 1:
      Mixed[I] = usubSat(Cur[I], C[I], LaneBits);
      break;
    case 2:
      Mixed[I] = saddSat(Cur[I], C[I], LaneBits);
      break;
    default:
      Mixed[I] = ssubSat(Cur[I], C[I], LaneBits);
      break;
    }
  }
  return packLanes(Mixed, LaneBits) ^ (V + (u32(O.B) | 1u));
}

uint32_t evalPackedVectorBitIntrinsic(uint32_t V, unsigned LaneBits,
                                      unsigned Which) {
  unsigned Lanes = 32 / LaneBits;
  auto Cur = unpackLanes(V, LaneBits);
  std::array<uint64_t, 8> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    switch (Which) {
    case 0:
      Mixed[I] = ctlzBits(Cur[I], LaneBits);
      break;
    case 1:
      Mixed[I] = cttzBits(Cur[I], LaneBits);
      break;
    case 2:
      Mixed[I] = ctpopBits(Cur[I], LaneBits);
      break;
    default:
      Mixed[I] = bitreverseBits(Cur[I], LaneBits);
      break;
    }
  }
  return V ^ packLanes(Mixed, LaneBits);
}

uint32_t evalPackedDynamicShift(uint32_t V, unsigned LaneBits, const Op &O) {
  unsigned Lanes = 32 / LaneBits;
  auto Cur = unpackLanes(V, LaneBits);
  auto Shift = unpackLanes(V ^ u32(O.A), LaneBits);
  std::array<uint64_t, 8> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    Shift[I] &= LaneBits - 1;
    if (Shift[I] == 0)
      Shift[I] = 1;
    switch (O.C % 3) {
    case 0:
      Mixed[I] = Cur[I] << Shift[I];
      break;
    case 1:
      Mixed[I] = Cur[I] >> Shift[I];
      break;
    default:
      Mixed[I] = ashrBits(Cur[I], LaneBits, Shift[I]);
      break;
    }
  }
  return packLanes(Mixed, LaneBits) + u32(O.B);
}

uint32_t evalPackedFshr(uint32_t V, unsigned LaneBits, const Op &O,
                        bool DynamicShift) {
  unsigned Lanes = 32 / LaneBits;
  auto Cur = unpackLanes(V, LaneBits);
  auto Other = packedConstantLanes(LaneBits, O.A);
  auto Shift = DynamicShift ? unpackLanes(V ^ u32(O.B), LaneBits)
                            : packedConstantLanes(LaneBits, O.C, true);
  std::array<uint64_t, 8> Mixed = {};
  for (unsigned I = 0; I < Lanes; ++I) {
    if (DynamicShift)
      Shift[I] &= LaneBits - 1;
    Mixed[I] = fshrBits(Other[I], Cur[I], Shift[I], LaneBits);
  }
  return packLanes(Mixed, LaneBits) + (V ^ (u32(O.C) | 1u));
}

uint32_t evalSmallSat(uint32_t V, const Op &O, unsigned Bits, unsigned Which,
                      bool SignedExtend) {
  uint64_t N = truncBits(V, Bits);
  uint64_t C = truncBits(O.A, Bits);
  uint64_t Mixed;
  switch (Which) {
  case 0:
    Mixed = uaddSat(N, C, Bits);
    break;
  case 1:
    Mixed = usubSat(N, C, Bits);
    break;
  case 2:
    Mixed = saddSat(N, C, Bits);
    break;
  default:
    Mixed = ssubSat(N, C, Bits);
    break;
  }
  uint32_t Extended = SignedExtend ? sextBits(Mixed, Bits) : zextBits(Mixed, Bits);
  return (V ^ (u32(O.B) | 1u)) + Extended;
}

uint32_t evalPrivateMemory(uint32_t V, uint32_t Idx, const Op &O) {
  std::array<uint32_t, 4> Values = {
      V,
      V + u32(O.A),
      V ^ u32(O.B),
      Idx * (u32(O.C) | 1u) + V,
  };
  return Values[O.C & 3u];
}

uint32_t localMemoryValue(uint32_t V, uint32_t Idx, const Op &O) {
  switch (O.C % 5) {
  case 0:
    return V + u32(O.A);
  case 1:
    return V ^ u32(O.B);
  case 2:
    return Idx * (u32(O.C) | 1u) + V;
  case 3:
    return V - u32(O.A);
  default:
    return (V & u32(O.A)) | (u32(O.B) & ~u32(O.A));
  }
}

uint32_t evalLocalMemory(uint32_t V, uint32_t Idx, const Op &O, unsigned Bits,
                         bool SignedExtend = false) {
  uint32_t Loaded = localMemoryValue(V, Idx, O);
  if (Bits < 32)
    Loaded = SignedExtend ? sextBits(Loaded, Bits) : zextBits(Loaded, Bits);
  return Loaded ^ (V + (u32(O.B) | 1u));
}

uint32_t evalLocalMemoryPair(uint32_t V, uint32_t Idx, const Op &O) {
  uint32_t First = localMemoryValue(V, Idx, O);
  uint32_t Second = (First + u32(O.B)) ^ (V * (u32(O.C) | 1u));
  return (First + Second) ^ (V + u32(O.A));
}

uint32_t atomicOperandValue(uint32_t V, uint32_t Idx, const Op &O) {
  return (V ^ u32(O.A)) + Idx * (u32(O.B) | 1u);
}

uint32_t applyAtomicRMW(uint32_t Old, uint32_t Operand, unsigned Which) {
  switch (Which) {
  case 0:
    return Old + Operand;
  case 1:
    return Old - Operand;
  case 2:
    return Old & Operand;
  case 3:
    return Old | Operand;
  case 4:
    return Old ^ Operand;
  case 5:
    return Operand;
  case 6:
    return static_cast<uint32_t>(sminBits(Old, Operand, 32));
  case 7:
    return static_cast<uint32_t>(smaxBits(Old, Operand, 32));
  case 8:
    return static_cast<uint32_t>(uminBits(Old, Operand, 32));
  default:
    return static_cast<uint32_t>(umaxBits(Old, Operand, 32));
  }
}

uint32_t evalAtomicRMW(uint32_t V, uint32_t Idx, const Op &O, unsigned Which) {
  uint32_t Old = localMemoryValue(V, Idx, O);
  uint32_t Operand = atomicOperandValue(V, Idx, O);
  uint32_t New = applyAtomicRMW(Old, Operand, Which);
  return (Old + New) ^ (V + (u32(O.C) | 1u));
}

uint32_t evalRotate(uint32_t V, const Op &O, bool Left) {
  uint32_t Shift = (V ^ u32(O.B)) & 31u;
  if (Left)
    Shift = adjustFshlShift(Shift);
  return static_cast<uint32_t>(
      Left ? fshlBits(V, u32(O.A), Shift, 32)
           : fshrBits(V, u32(O.A), Shift, 32));
}

uint32_t evalNarrowBitIntrinsic(uint32_t V, unsigned Bits, unsigned Which,
                                bool SignedExtend = false) {
  uint64_t N = truncBits(V, Bits);
  uint64_t Mixed;
  switch (Which) {
  case 0:
    Mixed = ctlzBits(N, Bits);
    break;
  case 1:
    Mixed = cttzBits(N, Bits);
    break;
  case 2:
    Mixed = ctpopBits(N, Bits);
    break;
  case 3:
    Mixed = bitreverseBits(N, Bits);
    break;
  default:
    Mixed = bswapBits(N, Bits);
    break;
  }
  uint32_t Extended = SignedExtend ? sextBits(Mixed, Bits) : zextBits(Mixed, Bits);
  return V ^ Extended;
}

uint32_t evalNarrowAbs(uint32_t V, unsigned Bits, bool SignedExtend) {
  uint64_t Mixed = absBits(V, Bits);
  uint32_t Extended = SignedExtend ? sextBits(Mixed, Bits) : zextBits(Mixed, Bits);
  return V + Extended;
}

uint32_t evalNarrowMinMax(uint32_t V, const Op &O, unsigned Bits,
                          unsigned Which, bool SignedExtend) {
  uint64_t N = truncBits(V, Bits);
  uint64_t C = truncBits(O.A, Bits);
  uint64_t Mixed;
  switch (Which) {
  case 0:
    Mixed = uminBits(N, C, Bits);
    break;
  case 1:
    Mixed = umaxBits(N, C, Bits);
    break;
  case 2:
    Mixed = sminBits(N, C, Bits);
    break;
  default:
    Mixed = smaxBits(N, C, Bits);
    break;
  }
  uint32_t Extended = SignedExtend ? sextBits(Mixed, Bits) : zextBits(Mixed, Bits);
  return (V + u32(O.B)) ^ Extended;
}

uint32_t evalWideCompareSelect(uint32_t V, const Op &O, bool SignedCompare) {
  uint64_t W = V;
  bool Cmp = SignedCompare ? (signedBits(W, 64) < signedBits(O.A, 64))
                           : (W < O.A);
  uint64_t Mixed = Cmp ? (W ^ O.B) : (W + O.C);
  return static_cast<uint32_t>(Mixed) ^ static_cast<uint32_t>(Mixed >> 32);
}

uint32_t evalWideMinMax(uint32_t V, const Op &O, unsigned Which) {
  uint64_t W = V;
  uint64_t Mixed;
  switch (Which) {
  case 0:
    Mixed = uminBits(W, O.A, 64);
    break;
  case 1:
    Mixed = umaxBits(W, O.A, 64);
    break;
  case 2:
    Mixed = sminBits(W, O.A, 64);
    break;
  default:
    Mixed = smaxBits(W, O.A, 64);
    break;
  }
  return (static_cast<uint32_t>(Mixed) ^ static_cast<uint32_t>(Mixed >> 32)) +
         u32(O.B);
}

uint32_t evalWideFshr(uint32_t V, const Op &O, bool DynamicShift) {
  uint64_t W = V;
  uint64_t Shift = DynamicShift ? ((W ^ O.B) & 63u) : (O.B & 63u);
  uint64_t Mixed = fshrBits(O.A, W, Shift, 64);
  return static_cast<uint32_t>(Mixed) ^ static_cast<uint32_t>(Mixed >> 32);
}

uint32_t nonzeroShift32(uint32_t Seed, unsigned Mask) {
  uint32_t Shift = Seed & Mask;
  return Shift == 0 ? 1 : Shift;
}

uint64_t nonzeroShift64(uint64_t Seed, unsigned Mask) {
  uint64_t Shift = Seed & Mask;
  return Shift == 0 ? 1 : Shift;
}

uint32_t evalDynamicShift32(uint32_t V, const Op &O, unsigned Which) {
  uint32_t Shift = nonzeroShift32(V ^ u32(O.A), 31);
  uint32_t Mixed;
  switch (Which) {
  case 0:
    Mixed = V << Shift;
    break;
  case 1:
    Mixed = V >> Shift;
    break;
  default:
    Mixed = static_cast<uint32_t>(ashrBits(V, 32, Shift));
    break;
  }
  return Mixed + u32(O.B);
}

uint32_t evalDynamicShift64(uint32_t V, const Op &O, unsigned Which) {
  uint64_t W = V;
  uint64_t Shift = nonzeroShift64(W ^ O.A, 63);
  uint64_t Mixed;
  switch (Which) {
  case 0:
    Mixed = W << Shift;
    break;
  case 1:
    Mixed = W >> Shift;
    break;
  default:
    Mixed = ashrBits(W, 64, Shift);
    break;
  }
  return static_cast<uint32_t>(Mixed) ^ static_cast<uint32_t>(Mixed >> 32);
}

uint32_t evalNarrowDynamicShift(uint32_t V, const Op &O, unsigned Bits,
                                unsigned Which, bool SignedExtend) {
  uint64_t N = truncBits(V, Bits);
  uint64_t Shift = nonzeroShift32(V ^ u32(O.A), Bits - 1);
  uint64_t Mixed;
  switch (Which) {
  case 0:
    Mixed = N << Shift;
    break;
  case 1:
    Mixed = N >> Shift;
    break;
  default:
    Mixed = ashrBits(N, Bits, Shift);
    break;
  }
  uint32_t Extended = SignedExtend ? sextBits(Mixed, Bits) : zextBits(Mixed, Bits);
  return (V + u32(O.B)) ^ Extended;
}

std::optional<uint32_t> evalOp(uint32_t V, uint32_t Idx, const Op &O) {
  switch (O.Kind) {
  case 0:
    return V + u32(O.A);
  case 1:
    return V - u32(O.A);
  case 2:
    return V * u32(O.A & 0xffffu);
  case 3:
    return V ^ u32(O.A);
  case 4:
    return V & u32(O.A);
  case 5:
    return V | u32(O.A);
  case 6:
    return V << (O.A & 31u);
  case 7:
    return V >> (O.A & 31u);
  case 8:
    return static_cast<uint32_t>(ashrBits(V, 32, O.A & 31u));
  case 9:
    return static_cast<uint32_t>(ctlzBits(V, 32));
  case 10:
    return static_cast<uint32_t>(cttzBits(V, 32));
  case 11:
    return static_cast<uint32_t>(ctpopBits(V, 32));
  case 12:
    return static_cast<uint32_t>(bswapBits(V, 32));
  case 13:
    return static_cast<uint32_t>(bitreverseBits(V, 32));
  case 14:
    return static_cast<uint32_t>(uminBits(V, u32(O.A), 32));
  case 15:
    return static_cast<uint32_t>(umaxBits(V, u32(O.A), 32));
  case 16:
    return static_cast<uint32_t>(sminBits(V, u32(O.A), 32));
  case 17:
    return static_cast<uint32_t>(smaxBits(V, u32(O.A), 32));
  case 18:
    return evalNarrow(V, 8, O);
  case 19:
    return evalNarrow(V, 16, O);
  case 20:
    return evalWide(V, O);
  case 21:
    return evalVector(V, 2, O);
  case 22:
    return evalVector(V, 4, O);
  case 23:
    return V < u32(O.A) ? (V ^ u32(O.B)) : (V + u32(O.C));
  case 24:
    return slt32(V, u32(O.A)) ? (V + u32(O.B)) : (V ^ u32(O.C));
  case 25:
    return V < u32(O.A) ? (V + u32(O.B)) : (V ^ u32(O.C));
  case 27:
    return evalNarrow(V, 8, O, true);
  case 28:
    return evalNarrow(V, 16, O, true);
  case 29:
    return static_cast<uint32_t>(
        fshlBits(V, u32(O.A), adjustFshlShift(O.B & 31u), 32));
  case 30:
    return static_cast<uint32_t>(fshrBits(u32(O.A), V, O.B & 31u, 32));
  case 31:
    return static_cast<uint32_t>(uaddSat(V, u32(O.A), 32));
  case 32:
    return static_cast<uint32_t>(usubSat(V, u32(O.A), 32));
  case 33:
    return static_cast<uint32_t>(saddSat(V, u32(O.A), 32));
  case 34:
    return static_cast<uint32_t>(ssubSat(V, u32(O.A), 32));
  case 35:
    return evalOverflow(V, O, 0);
  case 36:
    return evalOverflow(V, O, 1);
  case 37:
    return evalOverflow(V, O, 2);
  case 38:
    return evalOverflow(V, O, 3);
  case 39:
    return evalOverflow(V, O, 4);
  case 40:
    return evalOverflow(V, O, 5);
  case 41:
    return static_cast<uint32_t>(absBits(V, 32));
  case 42:
    return static_cast<uint32_t>(
        fshlBits(V, u32(O.A), adjustFshlShift(V & 31u), 32));
  case 43:
    return evalPackedVector(V, 8, O);
  case 44:
    return evalPackedVector(V, 16, O);
  case 45:
    return evalSmallSat(V, O, 8, 0, false);
  case 46:
    return evalSmallSat(V, O, 8, 1, false);
  case 47:
    return evalSmallSat(V, O, 8, 2, true);
  case 48:
    return evalSmallSat(V, O, 8, 3, true);
  case 49:
    return evalSmallSat(V, O, 16, 0, false);
  case 50:
    return evalSmallSat(V, O, 16, 1, false);
  case 51:
    return evalSmallSat(V, O, 16, 2, true);
  case 52:
    return evalSmallSat(V, O, 16, 3, true);
  case 53:
    return evalPrivateMemory(V, Idx, O);
  case 54:
    return evalRotate(V, O, true);
  case 55:
    return evalRotate(V, O, false);
  case 56: {
    bool Cmp = ((V & (u32(O.A) | 1u)) == (u32(O.B) & (u32(O.A) | 1u)));
    return Cmp ? evalPackedVector(V, 8, O) : evalSmallSat(V, O, 16, 0, false);
  }
  case 57:
    return slt32(V ^ u32(O.A), u32(O.B)) ? evalPackedVector(V, 16, O)
                                         : evalSmallSat(V, O, 8, 3, true);
  case 58:
    return evalRotate(evalPrivateMemory(V, Idx, O), O, true);
  case 59:
    return evalNarrowBitIntrinsic(V, 8, 0);
  case 60:
    return evalNarrowBitIntrinsic(V, 8, 1);
  case 61:
    return evalNarrowBitIntrinsic(V, 8, 2);
  case 62:
    return evalNarrowBitIntrinsic(V, 8, 3);
  case 63:
    return evalNarrowAbs(V, 8, false);
  case 64:
    return evalNarrowBitIntrinsic(V, 16, 0);
  case 65:
    return evalNarrowBitIntrinsic(V, 16, 1);
  case 66:
    return evalNarrowBitIntrinsic(V, 16, 2);
  case 67:
    return evalNarrowBitIntrinsic(V, 16, 3);
  case 68:
    return evalNarrowBitIntrinsic(V, 16, 4);
  case 69:
    return evalNarrowAbs(V, 16, true);
  case 70:
    return evalWideOverflow(V, O, 0);
  case 71:
    return evalWideOverflow(V, O, 1);
  case 72:
    return evalWideOverflow(V, O, 2);
  case 73:
    return evalWideOverflow(V, O, 3);
  case 74:
    return evalWideOverflow(V, O, 4);
  case 75:
    return evalNarrowMinMax(V, O, 8, 0, false);
  case 76:
    return evalNarrowMinMax(V, O, 8, 1, false);
  case 77:
    return evalNarrowMinMax(V, O, 8, 2, true);
  case 78:
    return evalNarrowMinMax(V, O, 8, 3, true);
  case 79:
    return evalNarrowMinMax(V, O, 16, 0, false);
  case 80:
    return evalNarrowMinMax(V, O, 16, 1, false);
  case 81:
    return evalNarrowMinMax(V, O, 16, 2, true);
  case 82:
    return evalNarrowMinMax(V, O, 16, 3, true);
  case 83:
    return evalWideCompareSelect(V, O, false);
  case 84:
    return evalWideCompareSelect(V, O, true);
  case 85:
    return evalWideMinMax(V, O, 0);
  case 86:
    return evalWideMinMax(V, O, 1);
  case 87:
    return evalWideMinMax(V, O, 2);
  case 88:
    return evalWideMinMax(V, O, 3);
  case 89:
    return evalWideFshr(V, O, false);
  case 90:
    return evalWideFshr(V, O, true);
  case 91:
    return evalDynamicShift32(V, O, 0);
  case 92:
    return evalDynamicShift32(V, O, 1);
  case 93:
    return evalDynamicShift32(V, O, 2);
  case 94:
    return evalDynamicShift64(V, O, 0);
  case 95:
    return evalDynamicShift64(V, O, 1);
  case 96:
    return evalDynamicShift64(V, O, 2);
  case 97:
    return evalNarrowDynamicShift(V, O, 8, 0, false);
  case 98:
    return evalNarrowDynamicShift(V, O, 8, 1, false);
  case 99:
    return evalNarrowDynamicShift(V, O, 8, 2, true);
  case 100:
    return evalNarrowDynamicShift(V, O, 16, 0, false);
  case 101:
    return evalNarrowDynamicShift(V, O, 16, 1, false);
  case 102:
    return evalNarrowDynamicShift(V, O, 16, 2, true);
  case 103:
    return evalPackedVectorSat(V, 8, O, 0);
  case 104:
    return evalPackedVectorSat(V, 8, O, 1);
  case 105:
    return evalPackedVectorSat(V, 8, O, 2);
  case 106:
    return evalPackedVectorSat(V, 8, O, 3);
  case 107:
    return evalPackedVectorSat(V, 16, O, 0);
  case 108:
    return evalPackedVectorSat(V, 16, O, 1);
  case 109:
    return evalPackedVectorSat(V, 16, O, 2);
  case 110:
    return evalPackedVectorSat(V, 16, O, 3);
  case 111:
    return evalPackedVectorBitIntrinsic(V, 8, 0);
  case 112:
    return evalPackedVectorBitIntrinsic(V, 8, 1);
  case 113:
    return evalPackedVectorBitIntrinsic(V, 8, 2);
  case 114:
    return evalPackedVectorBitIntrinsic(V, 8, 3);
  case 115:
    return evalPackedVectorBitIntrinsic(V, 16, 2);
  case 116:
    return evalPackedVectorBitIntrinsic(V, 16, 3);
  case 117:
    return evalPackedDynamicShift(V, 8, O);
  case 118:
    return evalPackedDynamicShift(V, 16, O);
  case 119:
    return evalI32VectorMinMax(V, 2, O, 0);
  case 120:
    return evalI32VectorMinMax(V, 2, O, 1);
  case 121:
    return evalI32VectorMinMax(V, 2, O, 2);
  case 122:
    return evalI32VectorMinMax(V, 2, O, 3);
  case 123:
    return evalI32VectorMinMax(V, 4, O, 0);
  case 124:
    return evalI32VectorMinMax(V, 4, O, 1);
  case 125:
    return evalI32VectorMinMax(V, 4, O, 2);
  case 126:
    return evalI32VectorMinMax(V, 4, O, 3);
  case 127:
    return evalI32VectorBitIntrinsic(V, 2, O, 0);
  case 128:
    return evalI32VectorBitIntrinsic(V, 4, O, 0);
  case 129:
    return evalI32VectorBitIntrinsic(V, 2, O, 1);
  case 130:
    return evalI32VectorBitIntrinsic(V, 4, O, 1);
  case 131:
    return evalI32VectorBitIntrinsic(V, 2, O, 2);
  case 132:
    return evalI32VectorBitIntrinsic(V, 4, O, 2);
  case 133:
    return evalI32VectorDynamicShift(V, 2, O);
  case 134:
    return evalI32VectorDynamicShift(V, 4, O);
  case 135:
    return evalPackedFshr(V, 8, O, false);
  case 136:
    return evalPackedFshr(V, 16, O, false);
  case 137:
    return evalPackedFshr(V, 8, O, true);
  case 138:
    return evalPackedFshr(V, 16, O, true);
  case 139:
    return evalI32VectorFshr(V, 2, O, false);
  case 140:
    return evalI32VectorFshr(V, 4, O, false);
  case 141:
    return evalI32VectorFshr(V, 2, O, true);
  case 142:
    return evalI32VectorFshr(V, 4, O, true);
  case 143:
    return evalDerivedI32VectorMix(V, 2, O);
  case 144:
    return evalDerivedI32VectorMix(V, 4, O);
  case 145:
    return evalDerivedI32VectorMinMax(V, 2, O, 0);
  case 146:
    return evalDerivedI32VectorMinMax(V, 2, O, 1);
  case 147:
    return evalDerivedI32VectorMinMax(V, 2, O, 2);
  case 148:
    return evalDerivedI32VectorMinMax(V, 2, O, 3);
  case 149:
    return evalDerivedI32VectorMinMax(V, 4, O, 0);
  case 150:
    return evalDerivedI32VectorMinMax(V, 4, O, 1);
  case 151:
    return evalDerivedI32VectorMinMax(V, 4, O, 2);
  case 152:
    return evalDerivedI32VectorMinMax(V, 4, O, 3);
  case 153:
    return evalDerivedI32VectorUnary(V, 2, O, 0);
  case 154:
    return evalDerivedI32VectorUnary(V, 4, O, 0);
  case 155:
    return evalDerivedI32VectorUnary(V, 2, O, 1);
  case 156:
    return evalDerivedI32VectorUnary(V, 4, O, 1);
  case 157:
    return evalDerivedI32VectorUnary(V, 2, O, 2);
  case 158:
    return evalDerivedI32VectorUnary(V, 4, O, 2);
  case 159:
  case 160:
  case 161:
  case 162:
  case 163:
  case 164:
  case 165:
  case 166:
  case 167:
  case 168:
  case 169:
  case 170:
  case 171:
  case 172:
    return std::nullopt;
  case 173:
    return evalLocalMemory(V, Idx, O, 32);
  case 174:
    return evalLocalMemory(V, Idx, O, 16);
  case 175:
    return evalLocalMemory(V, Idx, O, 16, true);
  case 176:
    return evalLocalMemory(V, Idx, O, 8);
  case 177:
    return evalLocalMemory(V, Idx, O, 8, true);
  case 178:
    return evalLocalMemoryPair(V, Idx, O);
  case 179:
    return evalLocalMemory(V, Idx, O, 32);
  case 180:
    return evalLocalMemory(V, Idx, O, 16);
  case 181:
    return evalLocalMemory(V, Idx, O, 16, true);
  case 182:
    return evalLocalMemory(V, Idx, O, 8);
  case 183:
    return evalLocalMemory(V, Idx, O, 8, true);
  case 184:
    return evalLocalMemoryPair(V, Idx, O);
  case 185:
  case 186:
  case 187:
  case 188:
  case 189:
  case 190:
  case 191:
  case 192:
  case 193:
  case 194:
    return evalAtomicRMW(V, Idx, O, O.Kind - 185);
  case 195:
  case 196:
  case 197:
  case 198:
  case 199:
  case 200:
  case 201:
  case 202:
  case 203:
  case 204:
    return evalAtomicRMW(V, Idx, O, O.Kind - 195);
  default:
    return slt32(V, u32(O.A)) ? (V ^ u32(O.B)) : (V - u32(O.C));
  }
}

std::optional<uint32_t> evalOps(uint32_t V, uint32_t Idx, ArrayRef<Op> Ops) {
  for (const Op &O : Ops) {
    auto Next = evalOp(V, Idx, O);
    if (!Next)
      return std::nullopt;
    V = *Next;
  }
  return V;
}

std::optional<uint32_t> evalProgramForInput(const Program &P, uint32_t Input,
                                            uint32_t Idx) {
  if (!P.UseStructuredCFG || P.Ops.size() < 4)
    return evalOps(Input, Idx, P.Ops);

  size_t Prefix = 0;
  size_t ThenLen = 0;
  size_t ElseLen = 0;
  size_t SuffixStart = 0;
  chooseStructuredSlices(P, Prefix, ThenLen, ElseLen, SuffixStart);

  auto PrefixValue = evalOps(Input, Idx, ArrayRef<Op>(P.Ops).take_front(Prefix));
  if (!PrefixValue)
    return std::nullopt;
  uint32_t PredBase = *PrefixValue ^ Idx;
  uint32_t PredicateConst = u32(P.CFGPredicate);
  bool Cond = false;
  switch ((P.CFGPredicate >> 32) & 3u) {
  case 0:
    Cond = PredBase < PredicateConst;
    break;
  case 1:
    Cond = slt32(PredBase, PredicateConst);
    break;
  case 2: {
    uint32_t Mask = u32(P.CFGPredicate >> 8) | 1u;
    Cond = ((PredBase & Mask) == (PredicateConst & Mask));
    break;
  }
  default: {
    uint32_t Mask = u32(P.CFGPredicate >> 16) | 1u;
    Cond = ((PredBase & Mask) != (PredicateConst & Mask));
    break;
  }
  }

  ArrayRef<Op> Ops(P.Ops);
  auto BranchValue =
      Cond ? evalOps(*PrefixValue, Idx, Ops.slice(Prefix, ThenLen))
           : evalOps(*PrefixValue, Idx, Ops.slice(Prefix + ThenLen, ElseLen));
  if (!BranchValue)
    return std::nullopt;
  return evalOps(*BranchValue, Idx, Ops.drop_front(SuffixStart));
}

std::optional<std::array<uint32_t, InputCount>>
evalProgramForInputs(const Program &P, ArrayRef<uint32_t> Inputs) {
  std::array<uint32_t, InputCount> Expected{};
  for (unsigned I = 0; I < InputCount; ++I) {
    auto Value = evalProgramForInput(P, Inputs[I], I);
    if (!Value)
      return std::nullopt;
    Expected[I] = *Value;
  }
  return Expected;
}

ConstantInt *ci32(LLVMContext &Ctx, uint32_t V) {
  return ConstantInt::get(Type::getInt32Ty(Ctx), V);
}

ConstantInt *ci64(LLVMContext &Ctx, uint64_t V) {
  return ConstantInt::get(Type::getInt64Ty(Ctx), V);
}

Constant *cf32(LLVMContext &Ctx, float V) {
  return ConstantFP::get(Type::getFloatTy(Ctx), V);
}

Constant *cf16(LLVMContext &Ctx, float V) {
  return ConstantFP::get(Type::getHalfTy(Ctx), V);
}

Constant *cf64(LLVMContext &Ctx, double V) {
  return ConstantFP::get(Type::getDoubleTy(Ctx), V);
}

float smallF16(uint64_t V) {
  return static_cast<float>(1u + static_cast<unsigned>(V & 0x0fu));
}

float smallF32(uint64_t V) {
  return static_cast<float>(1u + static_cast<unsigned>(V & 0xffu));
}

double smallF64(uint64_t V) {
  return static_cast<double>(1u + static_cast<unsigned>(V & 0xffffu));
}

bool allowM015ScalarFshlZero() {
  static const bool Allow =
      envFlag("FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO", false);
  return Allow;
}

Value *suppressM015FshlZeroShift(IRBuilder<> &B, Value *Shift) {
  if (allowM015ScalarFshlZero())
    return Shift;

  LLVMContext &Ctx = B.getContext();
  if (auto *CI = dyn_cast<ConstantInt>(Shift))
    return CI->isZero() ? ci32(Ctx, 1) : Shift;

  Value *IsZero = B.CreateICmpEQ(Shift, ci32(Ctx, 0));
  return B.CreateSelect(IsZero, ci32(Ctx, 1), Shift);
}

Constant *vectorConstant(LLVMContext &Ctx, ArrayRef<uint32_t> Values,
                         bool ShiftAmounts = false) {
  SmallVector<Constant *, 4> Constants;
  for (uint32_t V : Values)
    Constants.push_back(ci32(Ctx, ShiftAmounts ? (V & 31u) : V));
  return ConstantVector::get(Constants);
}

Constant *f32VectorConstant(LLVMContext &Ctx, ArrayRef<float> Values) {
  SmallVector<Constant *, 4> Constants;
  for (float V : Values)
    Constants.push_back(cf32(Ctx, V));
  return ConstantVector::get(Constants);
}

Constant *f16VectorConstant(LLVMContext &Ctx, ArrayRef<float> Values) {
  SmallVector<Constant *, 4> Constants;
  for (float V : Values)
    Constants.push_back(cf16(Ctx, V));
  return ConstantVector::get(Constants);
}

Value *emitNarrow(IRBuilder<> &B, Value *V, unsigned Bits, const Op &O,
                  bool SignedExtend = false) {
  LLVMContext &Ctx = B.getContext();
  Type *NarrowTy = Type::getIntNTy(Ctx, Bits);
  uint32_t Mask = (1u << Bits) - 1u;
  unsigned Shift = static_cast<unsigned>(O.B) & (Bits - 1);
  Value *N = B.CreateTrunc(V, NarrowTy);
  Value *Mixed;
  switch (O.C % 7) {
  case 0:
    Mixed = B.CreateAdd(N, ConstantInt::get(NarrowTy, O.A & Mask));
    break;
  case 1:
    Mixed = B.CreateSub(N, ConstantInt::get(NarrowTy, O.A & Mask));
    break;
  case 2:
    Mixed = B.CreateMul(N, ConstantInt::get(NarrowTy, O.A & Mask));
    break;
  case 3:
    Mixed = B.CreateXor(N, ConstantInt::get(NarrowTy, O.A & Mask));
    break;
  case 4:
    Mixed = B.CreateShl(N, ConstantInt::get(NarrowTy, Shift));
    break;
  case 5:
    Mixed = B.CreateLShr(N, ConstantInt::get(NarrowTy, Shift));
    break;
  default:
    Mixed = B.CreateAShr(N, ConstantInt::get(NarrowTy, Shift));
    break;
  }
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Extended = SignedExtend ? B.CreateSExt(Mixed, I32)
                                 : B.CreateZExt(Mixed, I32);
  return B.CreateXor(V, Extended);
}

Value *emitWide(IRBuilder<> &B, Module &M, Value *V, const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Value *W = B.CreateZExt(V, I64);
  unsigned Shift = static_cast<unsigned>(O.B) & 63u;
  Value *Mixed;
  switch (O.C % 13) {
  case 0:
    Mixed = B.CreateAdd(W, ci64(Ctx, O.A));
    break;
  case 1:
    Mixed = B.CreateSub(W, ci64(Ctx, O.A));
    break;
  case 2:
    Mixed = B.CreateMul(W, ci64(Ctx, O.A));
    break;
  case 3:
    Mixed = B.CreateXor(W, ci64(Ctx, O.A));
    break;
  case 4:
    Mixed = B.CreateAnd(W, ci64(Ctx, O.A));
    break;
  case 5:
    Mixed = B.CreateOr(W, ci64(Ctx, O.A));
    break;
  case 6:
    Mixed = B.CreateShl(W, ConstantInt::get(I64, Shift));
    break;
  case 7:
    Mixed = B.CreateLShr(W, ConstantInt::get(I64, Shift));
    break;
  case 8:
    Mixed = B.CreateAShr(W, ConstantInt::get(I64, Shift));
    break;
  case 9:
    Mixed = B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctlz, {I64}),
                         {W, ConstantInt::getFalse(Ctx)});
    break;
  case 10:
    Mixed = B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::cttz, {I64}),
                         {W, ConstantInt::getFalse(Ctx)});
    break;
  case 11:
    Mixed = B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctpop, {I64}),
                         {W});
    break;
  default:
    Mixed = B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bswap, {I64}),
                         {W});
    break;
  }
  return B.CreateAdd(V, B.CreateTrunc(Mixed, Type::getInt32Ty(Ctx)));
}

Value *emitOverflow(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                    Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Pair =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {I32}),
                   {V, ci32(Ctx, u32(O.A))});
  Value *Wrapped = B.CreateExtractValue(Pair, {0});
  Value *Overflow = B.CreateZExt(B.CreateExtractValue(Pair, {1}), I32);
  Value *Shifted = B.CreateShl(Overflow, ci32(Ctx, O.B & 31u));
  return B.CreateXor(Wrapped, Shifted);
}

Value *emitWideOverflow(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                        Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *W = B.CreateZExt(V, I64);
  Value *Pair =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {I64}),
                   {W, ci64(Ctx, O.A)});
  Value *Wrapped = B.CreateExtractValue(Pair, {0});
  Value *Overflow = B.CreateZExt(B.CreateExtractValue(Pair, {1}), I32);
  Value *Lo = B.CreateTrunc(Wrapped, I32);
  Value *Hi = B.CreateTrunc(B.CreateLShr(Wrapped, ci64(Ctx, 32)), I32);
  return B.CreateXor(B.CreateAdd(Lo, Hi),
                     B.CreateShl(Overflow, ci32(Ctx, O.B & 31u)));
}

Value *emitVector(IRBuilder<> &B, Value *V, unsigned Lanes, const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = ConstantAggregateZero::get(VecTy);
  std::array<uint32_t, 4> Init = {u32(O.A), u32(O.B), u32(O.C),
                                  u32(O.A + O.B)};
  for (unsigned I = 0; I < Lanes; ++I) {
    Value *LaneValue = I == 0 ? V : ci32(Ctx, Init[I - 1]);
    Cur = B.CreateInsertElement(Cur, LaneValue, ci32(Ctx, I));
  }

  std::array<uint32_t, 4> Consts = {u32(O.A >> 32), u32(O.B >> 32),
                                    u32(O.C >> 32), u32(O.A ^ O.C)};
  std::array<uint32_t, 4> Alts = {u32(~O.A), u32(~O.B), u32(~O.C),
                                  u32(O.A + O.B + O.C)};
  ArrayRef<uint32_t> C(Consts.data(), Lanes);
  Value *Mixed;
  switch ((O.A ^ O.C) % 8) {
  case 0:
    Mixed = B.CreateAdd(Cur, vectorConstant(Ctx, C));
    break;
  case 1:
    Mixed = B.CreateSub(Cur, vectorConstant(Ctx, C));
    break;
  case 2:
    Mixed = B.CreateMul(Cur, vectorConstant(Ctx, C));
    break;
  case 3:
    Mixed = B.CreateXor(Cur, vectorConstant(Ctx, C));
    break;
  case 4:
    Mixed = B.CreateAnd(Cur, vectorConstant(Ctx, C));
    break;
  case 5:
    Mixed = B.CreateOr(Cur, vectorConstant(Ctx, C));
    break;
  case 6:
    Mixed = B.CreateShl(Cur, vectorConstant(Ctx, C, true));
    break;
  default: {
    Value *Cmp = B.CreateICmpULT(Cur, vectorConstant(Ctx, C));
    Value *Alt = B.CreateXor(Cur, vectorConstant(Ctx, ArrayRef<uint32_t>(Alts.data(), Lanes)));
    Mixed = B.CreateSelect(Cmp, Alt, Cur);
    break;
  }
  }
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateXor(V, Extracted);
}

Value *makeI32Vector(IRBuilder<> &B, Value *V, unsigned Lanes, const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = ConstantAggregateZero::get(VecTy);
  std::array<uint32_t, 4> Init = {u32(O.A), u32(O.B), u32(O.C),
                                  u32(O.A + O.B + O.C)};
  for (unsigned I = 0; I < Lanes; ++I) {
    Value *LaneValue = I == 0 ? V : ci32(Ctx, Init[I - 1]);
    Cur = B.CreateInsertElement(Cur, LaneValue, ci32(Ctx, I));
  }
  return Cur;
}

Value *emitI32VectorMinMax(IRBuilder<> &B, Module &M, Value *V, unsigned Lanes,
                           const Op &O, Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = makeI32Vector(B, V, Lanes, O);
  std::array<uint32_t, 4> Consts = {u32(O.A >> 32), u32(O.B >> 32),
                                    u32(O.C >> 32), u32(O.A ^ O.C)};
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                   {Cur, vectorConstant(Ctx, ArrayRef<uint32_t>(Consts.data(),
                                                                Lanes))});
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateAdd(Extracted, B.CreateXor(V, ci32(Ctx, u32(O.C) | 1u)));
}

Value *emitI32VectorBitIntrinsic(IRBuilder<> &B, Module &M, Value *V,
                                 unsigned Lanes, const Op &O,
                                 Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = makeI32Vector(B, V, Lanes, O);
  FunctionCallee Fn = Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy});
  Value *Mixed;
  if (ID == Intrinsic::ctlz || ID == Intrinsic::cttz)
    Mixed = B.CreateCall(Fn, {Cur, ConstantInt::getFalse(Ctx)});
  else
    Mixed = B.CreateCall(Fn, {Cur});
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateXor(Extracted, B.CreateAdd(V, ci32(Ctx, u32(O.A) | 1u)));
}

Value *emitI32VectorDynamicShift(IRBuilder<> &B, Value *V, unsigned Lanes,
                                 const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = makeI32Vector(B, V, Lanes, O);
  std::array<uint32_t, 4> ShiftMasks = {31, 31, 31, 31};
  Value *Shift =
      B.CreateAnd(Cur, vectorConstant(Ctx, ArrayRef<uint32_t>(ShiftMasks.data(),
                                                              Lanes)));
  Value *IsZero =
      B.CreateICmpEQ(Shift, ConstantAggregateZero::get(VecTy));
  Shift = B.CreateSelect(IsZero,
                         ConstantVector::getSplat(ElementCount::getFixed(Lanes),
                                                  ci32(Ctx, 1)),
                         Shift);
  Value *Mixed;
  switch (O.C % 3) {
  case 0:
    Mixed = B.CreateShl(Cur, Shift);
    break;
  case 1:
    Mixed = B.CreateLShr(Cur, Shift);
    break;
  default:
    Mixed = B.CreateAShr(Cur, Shift);
    break;
  }
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateAdd(Extracted, ci32(Ctx, u32(O.A)));
}

Value *emitI32VectorFshr(IRBuilder<> &B, Module &M, Value *V, unsigned Lanes,
                         const Op &O, bool DynamicShift) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = makeI32Vector(B, V, Lanes, O);
  std::array<uint32_t, 4> Consts = {u32(O.A), u32(O.B), u32(O.C),
                                    u32(O.A ^ O.C)};
  Value *Other =
      vectorConstant(Ctx, ArrayRef<uint32_t>(Consts.data(), Lanes));
  Value *Shift;
  if (DynamicShift) {
    std::array<uint32_t, 4> ShiftMasks = {31, 31, 31, 31};
    Shift = B.CreateAnd(Cur, vectorConstant(Ctx, ArrayRef<uint32_t>(
                                                   ShiftMasks.data(), Lanes)));
  } else {
    Shift = vectorConstant(Ctx, ArrayRef<uint32_t>(Consts.data(), Lanes), true);
  }
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {VecTy}),
                   {Other, Cur, Shift});
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateXor(Extracted, B.CreateAdd(V, ci32(Ctx, u32(O.C) | 1u)));
}

Value *makeDerivedI32Vector(IRBuilder<> &B, Value *V, unsigned Lanes,
                            const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = ConstantAggregateZero::get(VecTy);
  for (unsigned I = 0; I < Lanes; ++I) {
    Value *LaneValue = V;
    switch (I) {
    case 1:
      LaneValue = B.CreateXor(V, ci32(Ctx, u32(O.A)));
      break;
    case 2:
      LaneValue = B.CreateAdd(V, ci32(Ctx, u32(O.B)));
      break;
    default:
      LaneValue = B.CreateOr(B.CreateLShr(V, ci32(Ctx, O.C & 31u)),
                             ci32(Ctx, u32(O.A >> 32)));
      break;
    }
    Cur = B.CreateInsertElement(Cur, LaneValue, ci32(Ctx, I));
  }
  return Cur;
}

Value *emitDerivedI32VectorMix(IRBuilder<> &B, Value *V, unsigned Lanes,
                               const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = makeDerivedI32Vector(B, V, Lanes, O);
  std::array<uint32_t, 4> Consts = {u32(O.A >> 32), u32(O.B >> 32),
                                    u32(O.C >> 32), u32(O.A ^ O.C)};
  ArrayRef<uint32_t> C(Consts.data(), Lanes);
  Value *Mixed;
  switch (O.C % 9) {
  case 0:
    Mixed = B.CreateAdd(Cur, vectorConstant(Ctx, C));
    break;
  case 1:
    Mixed = B.CreateSub(Cur, vectorConstant(Ctx, C));
    break;
  case 2:
    Mixed = B.CreateMul(Cur, vectorConstant(Ctx, C));
    break;
  case 3:
    Mixed = B.CreateXor(Cur, vectorConstant(Ctx, C));
    break;
  case 4:
    Mixed = B.CreateAnd(Cur, vectorConstant(Ctx, C));
    break;
  case 5:
    Mixed = B.CreateOr(Cur, vectorConstant(Ctx, C));
    break;
  case 6:
    Mixed = B.CreateShl(Cur, vectorConstant(Ctx, C, true));
    break;
  case 7:
    Mixed = B.CreateLShr(Cur, vectorConstant(Ctx, C, true));
    break;
  default: {
    Value *Shift =
        B.CreateAnd(Cur, ConstantVector::getSplat(ElementCount::getFixed(Lanes),
                                                  ci32(Ctx, 31)));
    Value *IsZero = B.CreateICmpEQ(Shift, ConstantAggregateZero::get(VecTy));
    Shift = B.CreateSelect(
        IsZero, ConstantVector::getSplat(ElementCount::getFixed(Lanes),
                                         ci32(Ctx, 1)),
        Shift);
    Mixed = B.CreateAShr(Cur, Shift);
    break;
  }
  }
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateXor(Extracted, B.CreateAdd(V, ci32(Ctx, u32(O.A) | 1u)));
}

Value *emitDerivedI32VectorMinMax(IRBuilder<> &B, Module &M, Value *V,
                                  unsigned Lanes, const Op &O,
                                  Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = makeDerivedI32Vector(B, V, Lanes, O);
  Value *Other =
      makeDerivedI32Vector(B, B.CreateXor(V, ci32(Ctx, u32(O.C))), Lanes, O);
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                   {Cur, Other});
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateAdd(Extracted, B.CreateXor(V, ci32(Ctx, u32(O.B) | 1u)));
}

Value *emitDerivedI32VectorUnaryIntrinsic(IRBuilder<> &B, Module &M, Value *V,
                                          unsigned Lanes, const Op &O,
                                          Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  Value *Cur = makeDerivedI32Vector(B, V, Lanes, O);
  FunctionCallee Fn = Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy});
  Value *Mixed = ID == Intrinsic::abs
                     ? B.CreateCall(Fn, {Cur, ConstantInt::getFalse(Ctx)})
                     : B.CreateCall(Fn, {Cur});
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateXor(Extracted, B.CreateAdd(V, ci32(Ctx, u32(O.C) | 1u)));
}

Value *boundedI32ForFP(IRBuilder<> &B, Value *V, const Op &O, bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Value *I = B.CreateXor(V, ci32(Ctx, u32(O.A)));
  if (!Signed)
    return B.CreateAnd(I, ci32(Ctx, 0xffffu));
  return B.CreateAShr(B.CreateShl(I, ci32(Ctx, 16)), ci32(Ctx, 16));
}

Value *boundedI32ForF16(IRBuilder<> &B, Value *V, const Op &O, bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Value *I = B.CreateXor(V, ci32(Ctx, u32(O.A)));
  if (!Signed)
    return B.CreateAnd(I, ci32(Ctx, 0x03ffu));
  return B.CreateAShr(B.CreateShl(I, ci32(Ctx, 21)), ci32(Ctx, 21));
}

Value *boundedI32VectorForFP(IRBuilder<> &B, Value *V, unsigned Lanes,
                             const Op &O, bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Value *I = makeDerivedI32Vector(B, V, Lanes, O);
  auto Splat = [&](uint32_t X) {
    return ConstantVector::getSplat(ElementCount::getFixed(Lanes),
                                    ci32(Ctx, X));
  };
  if (!Signed)
    return B.CreateAnd(I, Splat(0xffffu));
  return B.CreateAShr(B.CreateShl(I, Splat(16)), Splat(16));
}

Value *boundedI32VectorForF16(IRBuilder<> &B, Value *V, unsigned Lanes,
                              const Op &O, bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Value *I = makeDerivedI32Vector(B, V, Lanes, O);
  auto Splat = [&](uint32_t X) {
    return ConstantVector::getSplat(ElementCount::getFixed(Lanes),
                                    ci32(Ctx, X));
  };
  if (!Signed)
    return B.CreateAnd(I, Splat(0x03ffu));
  return B.CreateAShr(B.CreateShl(I, Splat(21)), Splat(21));
}

Value *emitF32ScalarMix(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                        bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  Value *I = boundedI32ForFP(B, V, O, Signed);
  Value *F = Signed ? B.CreateSIToFP(I, F32) : B.CreateUIToFP(I, F32);
  Value *C0 = cf32(Ctx, smallF32(O.B));
  Value *C1 = cf32(Ctx, smallF32(O.C));
  Value *Mixed;
  switch (O.C % 10) {
  case 0:
    Mixed = B.CreateFAdd(F, C0);
    break;
  case 1:
    Mixed = B.CreateFSub(F, C0);
    break;
  case 2:
    Mixed = B.CreateFMul(F, C0);
    break;
  case 3:
    Mixed = B.CreateFNeg(F);
    break;
  case 4:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {F32}), {F});
    break;
  case 5:
    Mixed = B.CreateFDiv(F, C0);
    break;
  case 6: {
    Value *Abs = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {F32}), {F});
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sqrt, {F32}), {Abs});
    break;
  }
  case 7:
    Mixed = B.CreateSelect(B.CreateFCmpOLT(F, C0), F, C0);
    break;
  case 8:
    Mixed = B.CreateSelect(B.CreateFCmpOGT(F, C0), F, C0);
    break;
  default:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fma, {F32}),
        {F, C0, C1});
    break;
  }
  return B.CreateXor(V, B.CreateBitCast(Mixed, I32));
}

Value *emitF64ScalarMix(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                        bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  Type *F64 = Type::getDoubleTy(Ctx);
  Value *I32Value = boundedI32ForFP(B, V, O, Signed);
  Value *I64Value =
      Signed ? B.CreateSExt(I32Value, I64) : B.CreateZExt(I32Value, I64);
  Value *F = Signed ? B.CreateSIToFP(I64Value, F64)
                    : B.CreateUIToFP(I64Value, F64);
  Value *C0 = cf64(Ctx, smallF64(O.B));
  Value *C1 = cf64(Ctx, smallF64(O.C));
  Value *Mixed;
  switch (O.C % 11) {
  case 0:
    Mixed = B.CreateFAdd(F, C0);
    break;
  case 1:
    Mixed = B.CreateFSub(F, C0);
    break;
  case 2:
    Mixed = B.CreateFMul(F, C0);
    break;
  case 3:
    Mixed = B.CreateFNeg(F);
    break;
  case 4:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {F64}), {F});
    break;
  case 5:
    Mixed = B.CreateFDiv(F, C0);
    break;
  case 6: {
    Value *Abs = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {F64}), {F});
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sqrt, {F64}), {Abs});
    break;
  }
  case 7:
    Mixed = B.CreateFPExt(B.CreateFPTrunc(F, F32), F64);
    break;
  case 8:
    Mixed = B.CreateSelect(B.CreateFCmpOLT(F, C0), F, C0);
    break;
  case 9:
    Mixed = B.CreateSelect(B.CreateFCmpOGT(F, C0), F, C0);
    break;
  default:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fma, {F64}),
        {F, C0, C1});
    break;
  }
  Value *Bits = B.CreateBitCast(Mixed, I64);
  Value *Lo = B.CreateTrunc(Bits, I32);
  Value *Hi = B.CreateTrunc(B.CreateLShr(Bits, ci64(Ctx, 32)), I32);
  return B.CreateXor(V, B.CreateXor(Lo, Hi));
}

Value *emitF32VectorMix(IRBuilder<> &B, Module &M, Value *V, unsigned Lanes,
                        const Op &O, bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  auto *VecTy = FixedVectorType::get(F32, Lanes);
  Value *I = boundedI32VectorForFP(B, V, Lanes, O, Signed);
  Value *F = Signed ? B.CreateSIToFP(I, VecTy) : B.CreateUIToFP(I, VecTy);
  std::array<float, 4> C0Values = {smallF32(O.A), smallF32(O.B),
                                   smallF32(O.C), smallF32(O.A ^ O.C)};
  std::array<float, 4> C1Values = {smallF32(O.C), smallF32(O.A),
                                   smallF32(O.B), smallF32(O.A + O.B)};
  Value *C0 = f32VectorConstant(Ctx, ArrayRef<float>(C0Values.data(), Lanes));
  Value *C1 = f32VectorConstant(Ctx, ArrayRef<float>(C1Values.data(), Lanes));
  Value *Mixed;
  switch (O.C % 10) {
  case 0:
    Mixed = B.CreateFAdd(F, C0);
    break;
  case 1:
    Mixed = B.CreateFSub(F, C0);
    break;
  case 2:
    Mixed = B.CreateFMul(F, C0);
    break;
  case 3:
    Mixed = B.CreateFNeg(F);
    break;
  case 4:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {VecTy}), {F});
    break;
  case 5:
    Mixed = B.CreateFDiv(F, C0);
    break;
  case 6: {
    Value *Abs = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {VecTy}), {F});
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sqrt, {VecTy}), {Abs});
    break;
  }
  case 7:
    Mixed = B.CreateSelect(B.CreateFCmpOLT(F, C0), F, C0);
    break;
  case 8:
    Mixed = B.CreateSelect(B.CreateFCmpOGT(F, C0), F, C0);
    break;
  default:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fma, {VecTy}),
        {F, C0, C1});
    break;
  }
  Value *Extracted = B.CreateExtractElement(Mixed, ci32(Ctx, O.B % Lanes));
  return B.CreateXor(V, B.CreateBitCast(Extracted, I32));
}

Value *emitF16ScalarMix(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                        bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F16 = Type::getHalfTy(Ctx);
  Value *I = boundedI32ForF16(B, V, O, Signed);
  Value *F = Signed ? B.CreateSIToFP(I, F16) : B.CreateUIToFP(I, F16);
  Value *C0 = cf16(Ctx, smallF16(O.B));
  Value *C1 = cf16(Ctx, smallF16(O.C));
  Value *Mixed;
  switch (O.C % 10) {
  case 0:
    Mixed = B.CreateFAdd(F, C0);
    break;
  case 1:
    Mixed = B.CreateFSub(F, C0);
    break;
  case 2:
    Mixed = B.CreateFMul(F, C0);
    break;
  case 3:
    Mixed = B.CreateFDiv(F, C0);
    break;
  case 4:
    Mixed = B.CreateFNeg(F);
    break;
  case 5:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {F16}), {F});
    break;
  case 6: {
    Value *Abs = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {F16}), {F});
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sqrt, {F16}), {Abs});
    break;
  }
  case 7:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fma, {F16}),
        {F, C0, C1});
    break;
  case 8:
    Mixed = B.CreateSelect(B.CreateFCmpOLT(F, C0), F, C0);
    break;
  default:
    Mixed = B.CreateSelect(B.CreateFCmpOGT(F, C0), F, C0);
    break;
  }
  return B.CreateXor(V, B.CreateZExt(B.CreateBitCast(Mixed, I16), I32));
}

Value *emitF16VectorMix(IRBuilder<> &B, Module &M, Value *V, unsigned Lanes,
                        const Op &O, bool Signed) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *F16 = Type::getHalfTy(Ctx);
  auto *VecTy = FixedVectorType::get(F16, Lanes);
  Value *I = boundedI32VectorForF16(B, V, Lanes, O, Signed);
  Value *F = Signed ? B.CreateSIToFP(I, VecTy) : B.CreateUIToFP(I, VecTy);
  std::array<float, 4> C0Values = {smallF16(O.A), smallF16(O.B),
                                   smallF16(O.C), smallF16(O.A ^ O.C)};
  std::array<float, 4> C1Values = {smallF16(O.C), smallF16(O.A),
                                   smallF16(O.B), smallF16(O.A + O.B)};
  Value *C0 = f16VectorConstant(Ctx, ArrayRef<float>(C0Values.data(), Lanes));
  Value *C1 = f16VectorConstant(Ctx, ArrayRef<float>(C1Values.data(), Lanes));
  Value *Mixed;
  switch (O.C % 10) {
  case 0:
    Mixed = B.CreateFAdd(F, C0);
    break;
  case 1:
    Mixed = B.CreateFSub(F, C0);
    break;
  case 2:
    Mixed = B.CreateFMul(F, C0);
    break;
  case 3:
    Mixed = B.CreateFDiv(F, C0);
    break;
  case 4:
    Mixed = B.CreateFNeg(F);
    break;
  case 5:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {VecTy}), {F});
    break;
  case 6: {
    Value *Abs = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fabs, {VecTy}), {F});
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sqrt, {VecTy}), {Abs});
    break;
  }
  case 7:
    Mixed = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fma, {VecTy}),
        {F, C0, C1});
    break;
  case 8:
    Mixed = B.CreateSelect(B.CreateFCmpOLT(F, C0), F, C0);
    break;
  default:
    Mixed = B.CreateSelect(B.CreateFCmpOGT(F, C0), F, C0);
    break;
  }
  if (Lanes == 2)
    return B.CreateXor(V, B.CreateBitCast(Mixed, I32));

  Value *Bits = B.CreateBitCast(Mixed, I64);
  Value *Lo = B.CreateTrunc(Bits, I32);
  Value *Hi = B.CreateTrunc(B.CreateLShr(Bits, ci64(Ctx, 32)), I32);
  return B.CreateXor(V, B.CreateXor(Lo, Hi));
}

Constant *packedVectorConstant(LLVMContext &Ctx, unsigned LaneBits,
                               unsigned Lanes, uint64_t Bits,
                               bool ShiftAmounts = false) {
  Type *LaneTy = Type::getIntNTy(Ctx, LaneBits);
  uint64_t Mask = (1ull << LaneBits) - 1ull;
  SmallVector<Constant *, 8> Constants;
  for (unsigned I = 0; I < Lanes; ++I) {
    uint64_t Lane = (Bits >> (I * LaneBits)) & Mask;
    if (ShiftAmounts)
      Lane &= LaneBits - 1;
    Constants.push_back(ConstantInt::get(LaneTy, Lane));
  }
  return ConstantVector::get(Constants);
}

Value *emitPackedVector(IRBuilder<> &B, Value *V, unsigned LaneBits,
                        const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  unsigned Lanes = 32 / LaneBits;
  auto *VecTy = FixedVectorType::get(Type::getIntNTy(Ctx, LaneBits), Lanes);
  Value *Cur = B.CreateBitCast(V, VecTy);
  Value *C0 = packedVectorConstant(Ctx, LaneBits, Lanes, O.A);
  Value *C1 = packedVectorConstant(Ctx, LaneBits, Lanes, O.B);
  Value *Shift = packedVectorConstant(Ctx, LaneBits, Lanes, O.B, true);
  Value *Mixed;
  switch (O.C % 11) {
  case 0:
    Mixed = B.CreateAdd(Cur, C0);
    break;
  case 1:
    Mixed = B.CreateSub(Cur, C0);
    break;
  case 2:
    Mixed = B.CreateMul(Cur, C0);
    break;
  case 3:
    Mixed = B.CreateXor(Cur, C0);
    break;
  case 4:
    Mixed = B.CreateAnd(Cur, C0);
    break;
  case 5:
    Mixed = B.CreateOr(Cur, C0);
    break;
  case 6:
    Mixed = B.CreateShl(Cur, Shift);
    break;
  case 7:
    Mixed = B.CreateLShr(Cur, Shift);
    break;
  case 8:
    Mixed = B.CreateAShr(Cur, Shift);
    break;
  case 9: {
    Value *Cmp = B.CreateICmpULT(Cur, C0);
    Mixed = B.CreateSelect(Cmp, B.CreateXor(Cur, C1), B.CreateAdd(Cur, C0));
    break;
  }
  default: {
    Value *Cmp = B.CreateICmpSLT(Cur, C0);
    Mixed = B.CreateSelect(Cmp, B.CreateSub(Cur, C1), B.CreateOr(Cur, C0));
    break;
  }
  }
  Value *Packed = B.CreateBitCast(Mixed, I32);
  return B.CreateAdd(Packed, B.CreateXor(V, ci32(Ctx, u32(O.C) | 1u)));
}

Value *emitPackedVectorSat(IRBuilder<> &B, Module &M, Value *V,
                           unsigned LaneBits, const Op &O, Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  unsigned Lanes = 32 / LaneBits;
  auto *VecTy = FixedVectorType::get(Type::getIntNTy(Ctx, LaneBits), Lanes);
  Value *Cur = B.CreateBitCast(V, VecTy);
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                   {Cur, packedVectorConstant(Ctx, LaneBits, Lanes, O.A)});
  Value *Packed = B.CreateBitCast(Mixed, I32);
  return B.CreateXor(Packed, B.CreateAdd(V, ci32(Ctx, u32(O.B) | 1u)));
}

Value *emitPackedVectorBitIntrinsic(IRBuilder<> &B, Module &M, Value *V,
                                    unsigned LaneBits, Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  unsigned Lanes = 32 / LaneBits;
  auto *VecTy = FixedVectorType::get(Type::getIntNTy(Ctx, LaneBits), Lanes);
  Value *Cur = B.CreateBitCast(V, VecTy);
  FunctionCallee Fn = Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy});
  Value *Mixed;
  if (ID == Intrinsic::ctlz || ID == Intrinsic::cttz)
    Mixed = B.CreateCall(Fn, {Cur, ConstantInt::getFalse(Ctx)});
  else
    Mixed = B.CreateCall(Fn, {Cur});
  return B.CreateXor(V, B.CreateBitCast(Mixed, I32));
}

Value *emitPackedDynamicShift(IRBuilder<> &B, Value *V, unsigned LaneBits,
                              const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *LaneTy = Type::getIntNTy(Ctx, LaneBits);
  unsigned Lanes = 32 / LaneBits;
  auto *VecTy = FixedVectorType::get(LaneTy, Lanes);
  Value *Cur = B.CreateBitCast(V, VecTy);
  Value *Shift = B.CreateBitCast(B.CreateXor(V, ci32(Ctx, u32(O.A))), VecTy);
  Shift = B.CreateAnd(Shift, ConstantVector::getSplat(
                                 ElementCount::getFixed(Lanes),
                                 ConstantInt::get(LaneTy, LaneBits - 1)));
  Value *IsZero =
      B.CreateICmpEQ(Shift, ConstantAggregateZero::get(VecTy));
  Shift = B.CreateSelect(IsZero,
                         ConstantVector::getSplat(ElementCount::getFixed(Lanes),
                                                  ConstantInt::get(LaneTy, 1)),
                         Shift);
  Value *Mixed;
  switch (O.C % 3) {
  case 0:
    Mixed = B.CreateShl(Cur, Shift);
    break;
  case 1:
    Mixed = B.CreateLShr(Cur, Shift);
    break;
  default:
    Mixed = B.CreateAShr(Cur, Shift);
    break;
  }
  return B.CreateAdd(B.CreateBitCast(Mixed, I32), ci32(Ctx, u32(O.B)));
}

Value *emitPackedFshr(IRBuilder<> &B, Module &M, Value *V, unsigned LaneBits,
                      const Op &O, bool DynamicShift) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *LaneTy = Type::getIntNTy(Ctx, LaneBits);
  unsigned Lanes = 32 / LaneBits;
  auto *VecTy = FixedVectorType::get(LaneTy, Lanes);
  Value *Cur = B.CreateBitCast(V, VecTy);
  Value *Other = packedVectorConstant(Ctx, LaneBits, Lanes, O.A);
  Value *Shift;
  if (DynamicShift) {
    Shift = B.CreateBitCast(B.CreateXor(V, ci32(Ctx, u32(O.B))), VecTy);
    Shift = B.CreateAnd(Shift, ConstantVector::getSplat(
                                   ElementCount::getFixed(Lanes),
                                   ConstantInt::get(LaneTy, LaneBits - 1)));
  } else {
    Shift = packedVectorConstant(Ctx, LaneBits, Lanes, O.C, true);
  }
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {VecTy}),
                   {Other, Cur, Shift});
  return B.CreateAdd(B.CreateBitCast(Mixed, I32),
                     B.CreateXor(V, ci32(Ctx, u32(O.C) | 1u)));
}

Value *emitSmallSat(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                    unsigned Bits, Intrinsic::ID ID, bool SignedExtend) {
  LLVMContext &Ctx = B.getContext();
  Type *NarrowTy = Type::getIntNTy(Ctx, Bits);
  Type *I32 = Type::getInt32Ty(Ctx);
  uint64_t Mask = (1ull << Bits) - 1ull;
  Value *N = B.CreateTrunc(V, NarrowTy);
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {NarrowTy}),
                   {N, ConstantInt::get(NarrowTy, O.A & Mask)});
  Value *Extended = SignedExtend ? B.CreateSExt(Mixed, I32)
                                 : B.CreateZExt(Mixed, I32);
  return B.CreateAdd(B.CreateXor(V, ci32(Ctx, u32(O.B) | 1u)), Extended);
}

Value *emitPrivateMemory(IRBuilder<> &B, Value *V, Value *Idx, const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *ArrTy = ArrayType::get(I32, 4);
  AllocaInst *Slot = B.CreateAlloca(ArrTy);
  std::array<Value *, 4> Values = {
      V,
      B.CreateAdd(V, ci32(Ctx, u32(O.A))),
      B.CreateXor(V, ci32(Ctx, u32(O.B))),
      B.CreateAdd(B.CreateMul(Idx, ci32(Ctx, u32(O.C) | 1u)), V),
  };
  for (unsigned I = 0; I < Values.size(); ++I) {
    Value *Ptr = B.CreateGEP(ArrTy, Slot, {ci32(Ctx, 0), ci32(Ctx, I)});
    B.CreateStore(Values[I], Ptr);
  }
  Value *LoadPtr = B.CreateGEP(ArrTy, Slot, {ci32(Ctx, 0), ci32(Ctx, O.C & 3u)});
  return B.CreateLoad(I32, LoadPtr);
}

GlobalVariable *getLocalMemoryArray(Module &M, Type *ElemTy, StringRef Name,
                                    unsigned AlignBytes) {
  auto *ArrTy = ArrayType::get(ElemTy, ThreadsPerBlock);
  if (auto *GV = M.getGlobalVariable(Name, true))
    return GV;
  auto *GV = new GlobalVariable(M, ArrTy, false, GlobalValue::InternalLinkage,
                                UndefValue::get(ArrTy), Name, nullptr,
                                GlobalValue::NotThreadLocal, 3);
  GV->setAlignment(Align(AlignBytes));
  return GV;
}

Value *localMemoryValue(IRBuilder<> &B, Value *V, Value *Idx, const Op &O) {
  LLVMContext &Ctx = B.getContext();
  switch (O.C % 5) {
  case 0:
    return B.CreateAdd(V, ci32(Ctx, u32(O.A)));
  case 1:
    return B.CreateXor(V, ci32(Ctx, u32(O.B)));
  case 2:
    return B.CreateAdd(B.CreateMul(Idx, ci32(Ctx, u32(O.C) | 1u)), V);
  case 3:
    return B.CreateSub(V, ci32(Ctx, u32(O.A)));
  default:
    return B.CreateOr(B.CreateAnd(V, ci32(Ctx, u32(O.A))),
                      ci32(Ctx, u32(O.B) & ~u32(O.A)));
  }
}

Value *localMemorySlot(IRBuilder<> &B, Value *Idx) {
  return B.CreateAnd(Idx, ci32(B.getContext(), ThreadsPerBlock - 1));
}

Value *emitLocalMemory(IRBuilder<> &B, Module &M, Value *V, Value *Idx,
                       const Op &O, unsigned Bits, bool SignedExtend = false) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *ElemTy = Type::getIntNTy(Ctx, Bits);
  auto *ArrTy = ArrayType::get(ElemTy, ThreadsPerBlock);
  GlobalVariable *LDS = getLocalMemoryArray(M, ElemTy,
                                            ("fuzzx_lds_i" + Twine(Bits)).str(),
                                            std::max(1u, Bits / 8));
  Value *Ptr = B.CreateGEP(ArrTy, LDS, {ci32(Ctx, 0), localMemorySlot(B, Idx)});
  Value *Stored = localMemoryValue(B, V, Idx, O);
  if (Bits < 32)
    Stored = B.CreateTrunc(Stored, ElemTy);
  B.CreateStore(Stored, Ptr);
  Value *Loaded = B.CreateLoad(ElemTy, Ptr);
  if (Bits < 32)
    Loaded =
        SignedExtend ? B.CreateSExt(Loaded, I32) : B.CreateZExt(Loaded, I32);
  return B.CreateXor(Loaded, B.CreateAdd(V, ci32(Ctx, u32(O.B) | 1u)));
}

Value *emitLocalMemoryPair(IRBuilder<> &B, Module &M, Value *V, Value *Idx,
                           const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *ArrTy = ArrayType::get(I32, ThreadsPerBlock);
  GlobalVariable *LDS = getLocalMemoryArray(M, I32, "fuzzx_lds_i32_pair", 4);
  Value *Ptr = B.CreateGEP(ArrTy, LDS, {ci32(Ctx, 0), localMemorySlot(B, Idx)});
  Value *First = localMemoryValue(B, V, Idx, O);
  B.CreateStore(First, Ptr);
  Value *FirstLoaded = B.CreateLoad(I32, Ptr);
  Value *Second = B.CreateXor(B.CreateAdd(FirstLoaded, ci32(Ctx, u32(O.B))),
                              B.CreateMul(V, ci32(Ctx, u32(O.C) | 1u)));
  B.CreateStore(Second, Ptr);
  Value *SecondLoaded = B.CreateLoad(I32, Ptr);
  return B.CreateXor(B.CreateAdd(FirstLoaded, SecondLoaded),
                     B.CreateAdd(V, ci32(Ctx, u32(O.A))));
}

Value *globalScratchPtr(IRBuilder<> &B, Value *Scratch, Value *Idx,
                        Type *ElemTy, unsigned Bits) {
  LLVMContext &Ctx = B.getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Value *Idx64 = B.CreateZExt(Idx, I64);
  Value *ElementIndex = Idx64;
  if (Bits < 32)
    ElementIndex = B.CreateMul(Idx64, ci64(Ctx, 32 / Bits));
  return B.CreateGEP(ElemTy, Scratch, ElementIndex);
}

Value *emitGlobalMemory(IRBuilder<> &B, Value *Scratch, Value *V, Value *Idx,
                        const Op &O, unsigned Bits,
                        bool SignedExtend = false) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *ElemTy = Type::getIntNTy(Ctx, Bits);
  Value *Ptr = globalScratchPtr(B, Scratch, Idx, ElemTy, Bits);
  Value *Stored = localMemoryValue(B, V, Idx, O);
  if (Bits < 32)
    Stored = B.CreateTrunc(Stored, ElemTy);
  B.CreateStore(Stored, Ptr);
  Value *Loaded = B.CreateLoad(ElemTy, Ptr);
  if (Bits < 32)
    Loaded =
        SignedExtend ? B.CreateSExt(Loaded, I32) : B.CreateZExt(Loaded, I32);
  return B.CreateXor(Loaded, B.CreateAdd(V, ci32(Ctx, u32(O.B) | 1u)));
}

Value *emitGlobalMemoryPair(IRBuilder<> &B, Value *Scratch, Value *V,
                            Value *Idx, const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Ptr = globalScratchPtr(B, Scratch, Idx, I32, 32);
  Value *First = localMemoryValue(B, V, Idx, O);
  B.CreateStore(First, Ptr);
  Value *FirstLoaded = B.CreateLoad(I32, Ptr);
  Value *Second = B.CreateXor(B.CreateAdd(FirstLoaded, ci32(Ctx, u32(O.B))),
                              B.CreateMul(V, ci32(Ctx, u32(O.C) | 1u)));
  B.CreateStore(Second, Ptr);
  Value *SecondLoaded = B.CreateLoad(I32, Ptr);
  return B.CreateXor(B.CreateAdd(FirstLoaded, SecondLoaded),
                     B.CreateAdd(V, ci32(Ctx, u32(O.A))));
}

AtomicRMWInst::BinOp atomicRMWBinOp(unsigned Which) {
  switch (Which) {
  case 0:
    return AtomicRMWInst::Add;
  case 1:
    return AtomicRMWInst::Sub;
  case 2:
    return AtomicRMWInst::And;
  case 3:
    return AtomicRMWInst::Or;
  case 4:
    return AtomicRMWInst::Xor;
  case 5:
    return AtomicRMWInst::Xchg;
  case 6:
    return AtomicRMWInst::Min;
  case 7:
    return AtomicRMWInst::Max;
  case 8:
    return AtomicRMWInst::UMin;
  default:
    return AtomicRMWInst::UMax;
  }
}

Value *atomicOperandValue(IRBuilder<> &B, Value *V, Value *Idx, const Op &O) {
  LLVMContext &Ctx = B.getContext();
  return B.CreateAdd(B.CreateXor(V, ci32(Ctx, u32(O.A))),
                     B.CreateMul(Idx, ci32(Ctx, u32(O.B) | 1u)));
}

Value *emitAtomicRMW(IRBuilder<> &B, Value *Ptr, Value *V, Value *Idx,
                     const Op &O, unsigned Which, StringRef Scope) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *OldValue = localMemoryValue(B, V, Idx, O);
  B.CreateStore(OldValue, Ptr);
  Value *Operand = atomicOperandValue(B, V, Idx, O);
  Value *Old = B.CreateAtomicRMW(atomicRMWBinOp(Which), Ptr, Operand, Align(4),
                                 AtomicOrdering::Monotonic,
                                 Ctx.getOrInsertSyncScopeID(Scope));
  Value *New = B.CreateLoad(I32, Ptr);
  return B.CreateXor(B.CreateAdd(Old, New),
                     B.CreateAdd(V, ci32(Ctx, u32(O.C) | 1u)));
}

Value *emitLocalAtomicRMW(IRBuilder<> &B, Module &M, Value *V, Value *Idx,
                          const Op &O, unsigned Which) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *ArrTy = ArrayType::get(I32, ThreadsPerBlock);
  GlobalVariable *LDS = getLocalMemoryArray(M, I32, "fuzzx_lds_i32_atomic", 4);
  Value *Ptr = B.CreateGEP(ArrTy, LDS, {ci32(Ctx, 0), localMemorySlot(B, Idx)});
  return emitAtomicRMW(B, Ptr, V, Idx, O, Which, "workgroup-one-as");
}

Value *emitGlobalAtomicRMW(IRBuilder<> &B, Value *Scratch, Value *V, Value *Idx,
                           const Op &O, unsigned Which) {
  Type *I32 = Type::getInt32Ty(B.getContext());
  Value *Ptr = globalScratchPtr(B, Scratch, Idx, I32, 32);
  return emitAtomicRMW(B, Ptr, V, Idx, O, Which, "agent-one-as");
}

Value *emitRotate(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                  Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Shift = B.CreateAnd(B.CreateXor(V, ci32(Ctx, u32(O.B))), ci32(Ctx, 31));
  if (ID == Intrinsic::fshl)
    Shift = suppressM015FshlZeroShift(B, Shift);
  return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {I32}),
                      {V, ci32(Ctx, u32(O.A)), Shift});
}

Value *emitNarrowBitIntrinsic(IRBuilder<> &B, Module &M, Value *V,
                              unsigned Bits, Intrinsic::ID ID,
                              bool SignedExtend = false) {
  LLVMContext &Ctx = B.getContext();
  Type *NarrowTy = Type::getIntNTy(Ctx, Bits);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *N = B.CreateTrunc(V, NarrowTy);
  FunctionCallee Fn = Intrinsic::getOrInsertDeclaration(&M, ID, {NarrowTy});
  Value *Mixed;
  if (ID == Intrinsic::ctlz || ID == Intrinsic::cttz)
    Mixed = B.CreateCall(Fn, {N, ConstantInt::getFalse(Ctx)});
  else
    Mixed = B.CreateCall(Fn, {N});
  Value *Extended = SignedExtend ? B.CreateSExt(Mixed, I32)
                                 : B.CreateZExt(Mixed, I32);
  return B.CreateXor(V, Extended);
}

Value *emitNarrowAbs(IRBuilder<> &B, Module &M, Value *V, unsigned Bits,
                     bool SignedExtend) {
  LLVMContext &Ctx = B.getContext();
  Type *NarrowTy = Type::getIntNTy(Ctx, Bits);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *N = B.CreateTrunc(V, NarrowTy);
  Value *Abs =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::abs,
                                                     {NarrowTy}),
                   {N, ConstantInt::getFalse(Ctx)});
  Value *Extended = SignedExtend ? B.CreateSExt(Abs, I32) : B.CreateZExt(Abs, I32);
  return B.CreateAdd(V, Extended);
}

Value *emitNarrowMinMax(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                        unsigned Bits, Intrinsic::ID ID, bool SignedExtend) {
  LLVMContext &Ctx = B.getContext();
  Type *NarrowTy = Type::getIntNTy(Ctx, Bits);
  Type *I32 = Type::getInt32Ty(Ctx);
  uint64_t Mask = (1ull << Bits) - 1ull;
  Value *N = B.CreateTrunc(V, NarrowTy);
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {NarrowTy}),
                   {N, ConstantInt::get(NarrowTy, O.A & Mask)});
  Value *Extended = SignedExtend ? B.CreateSExt(Mixed, I32)
                                 : B.CreateZExt(Mixed, I32);
  return B.CreateXor(B.CreateAdd(V, ci32(Ctx, u32(O.B))), Extended);
}

Value *emitWideCompareSelect(IRBuilder<> &B, Value *V, const Op &O,
                             bool SignedCompare) {
  LLVMContext &Ctx = B.getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *W = B.CreateZExt(V, I64);
  Value *Cmp = SignedCompare ? B.CreateICmpSLT(W, ci64(Ctx, O.A))
                             : B.CreateICmpULT(W, ci64(Ctx, O.A));
  Value *T = B.CreateXor(W, ci64(Ctx, O.B));
  Value *F = B.CreateAdd(W, ci64(Ctx, O.C));
  Value *Mixed = B.CreateSelect(Cmp, T, F);
  Value *Lo = B.CreateTrunc(Mixed, I32);
  Value *Hi = B.CreateTrunc(B.CreateLShr(Mixed, ci64(Ctx, 32)), I32);
  return B.CreateXor(Lo, Hi);
}

Value *emitWideMinMax(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                      Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *W = B.CreateZExt(V, I64);
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {I64}),
                   {W, ci64(Ctx, O.A)});
  Value *Lo = B.CreateTrunc(Mixed, I32);
  Value *Hi = B.CreateTrunc(B.CreateLShr(Mixed, ci64(Ctx, 32)), I32);
  return B.CreateAdd(B.CreateXor(Lo, Hi), ci32(Ctx, u32(O.B)));
}

Value *emitWideFshr(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                    bool DynamicShift) {
  LLVMContext &Ctx = B.getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *W = B.CreateZExt(V, I64);
  Value *Shift = DynamicShift
                     ? B.CreateAnd(B.CreateXor(W, ci64(Ctx, O.B)), ci64(Ctx, 63))
                     : ci64(Ctx, O.B & 63u);
  Value *Mixed =
      B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I64}),
                   {ci64(Ctx, O.A), W, Shift});
  Value *Lo = B.CreateTrunc(Mixed, I32);
  Value *Hi = B.CreateTrunc(B.CreateLShr(Mixed, ci64(Ctx, 32)), I32);
  return B.CreateXor(Lo, Hi);
}

Value *nonzeroMaskedShift32(IRBuilder<> &B, Value *Seed, unsigned Mask) {
  LLVMContext &Ctx = B.getContext();
  Value *Shift = B.CreateAnd(Seed, ci32(Ctx, Mask));
  return B.CreateSelect(B.CreateICmpEQ(Shift, ci32(Ctx, 0)), ci32(Ctx, 1),
                        Shift);
}

Value *nonzeroMaskedShift64(IRBuilder<> &B, Value *Seed, unsigned Mask) {
  LLVMContext &Ctx = B.getContext();
  Value *Shift = B.CreateAnd(Seed, ci64(Ctx, Mask));
  return B.CreateSelect(B.CreateICmpEQ(Shift, ci64(Ctx, 0)), ci64(Ctx, 1),
                        Shift);
}

Value *emitDynamicShift32(IRBuilder<> &B, Value *V, const Op &O, unsigned Which) {
  LLVMContext &Ctx = B.getContext();
  Value *Shift = nonzeroMaskedShift32(B, B.CreateXor(V, ci32(Ctx, u32(O.A))), 31);
  Value *Mixed;
  switch (Which) {
  case 0:
    Mixed = B.CreateShl(V, Shift);
    break;
  case 1:
    Mixed = B.CreateLShr(V, Shift);
    break;
  default:
    Mixed = B.CreateAShr(V, Shift);
    break;
  }
  return B.CreateAdd(Mixed, ci32(Ctx, u32(O.B)));
}

Value *emitDynamicShift64(IRBuilder<> &B, Value *V, const Op &O, unsigned Which) {
  LLVMContext &Ctx = B.getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *W = B.CreateZExt(V, I64);
  Value *Shift = nonzeroMaskedShift64(B, B.CreateXor(W, ci64(Ctx, O.A)), 63);
  Value *Mixed;
  switch (Which) {
  case 0:
    Mixed = B.CreateShl(W, Shift);
    break;
  case 1:
    Mixed = B.CreateLShr(W, Shift);
    break;
  default:
    Mixed = B.CreateAShr(W, Shift);
    break;
  }
  Value *Lo = B.CreateTrunc(Mixed, I32);
  Value *Hi = B.CreateTrunc(B.CreateLShr(Mixed, ci64(Ctx, 32)), I32);
  return B.CreateXor(Lo, Hi);
}

Value *emitNarrowDynamicShift(IRBuilder<> &B, Value *V, const Op &O,
                              unsigned Bits, unsigned Which,
                              bool SignedExtend) {
  LLVMContext &Ctx = B.getContext();
  Type *NarrowTy = Type::getIntNTy(Ctx, Bits);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *N = B.CreateTrunc(V, NarrowTy);
  Value *Shift32 =
      nonzeroMaskedShift32(B, B.CreateXor(V, ci32(Ctx, u32(O.A))), Bits - 1);
  Value *Shift = B.CreateTrunc(Shift32, NarrowTy);
  Value *Mixed;
  switch (Which) {
  case 0:
    Mixed = B.CreateShl(N, Shift);
    break;
  case 1:
    Mixed = B.CreateLShr(N, Shift);
    break;
  default:
    Mixed = B.CreateAShr(N, Shift);
    break;
  }
  Value *Extended = SignedExtend ? B.CreateSExt(Mixed, I32)
                                 : B.CreateZExt(Mixed, I32);
  return B.CreateXor(B.CreateAdd(V, ci32(Ctx, u32(O.B))), Extended);
}

Value *emitOp(IRBuilder<> &B, Module &M, Value *V, Value *Idx, Value *Scratch,
              const Op &O) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  switch (O.Kind) {
  case 0:
    return B.CreateAdd(V, ci32(Ctx, u32(O.A)));
  case 1:
    return B.CreateSub(V, ci32(Ctx, u32(O.A)));
  case 2:
    return B.CreateMul(V, ci32(Ctx, u32(O.A & 0xffffu)));
  case 3:
    return B.CreateXor(V, ci32(Ctx, u32(O.A)));
  case 4:
    return B.CreateAnd(V, ci32(Ctx, u32(O.A)));
  case 5:
    return B.CreateOr(V, ci32(Ctx, u32(O.A)));
  case 6:
    return B.CreateShl(V, ci32(Ctx, O.A & 31u));
  case 7:
    return B.CreateLShr(V, ci32(Ctx, O.A & 31u));
  case 8:
    return B.CreateAShr(V, ci32(Ctx, O.A & 31u));
  case 9:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctlz, {I32}),
                        {V, ConstantInt::getFalse(Ctx)});
  case 10:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::cttz, {I32}),
                        {V, ConstantInt::getFalse(Ctx)});
  case 11:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctpop, {I32}),
                        {V});
  case 12:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bswap, {I32}),
                        {V});
  case 13:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bitreverse, {I32}), {V});
  case 14:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umin, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 15:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umax, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 16:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smin, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 17:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smax, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 18:
    return emitNarrow(B, V, 8, O);
  case 19:
    return emitNarrow(B, V, 16, O);
  case 20:
    return emitWide(B, M, V, O);
  case 21:
    return emitVector(B, V, 2, O);
  case 22:
    return emitVector(B, V, 4, O);
  case 23: {
    Value *Cmp = B.CreateICmpULT(V, ci32(Ctx, u32(O.A)));
    Value *T = B.CreateXor(V, ci32(Ctx, u32(O.B)));
    Value *F = B.CreateAdd(V, ci32(Ctx, u32(O.C)));
    return B.CreateSelect(Cmp, T, F);
  }
  case 24: {
    Value *Cmp = B.CreateICmpSLT(V, ci32(Ctx, u32(O.A)));
    Value *T = B.CreateAdd(V, ci32(Ctx, u32(O.B)));
    Value *F = B.CreateXor(V, ci32(Ctx, u32(O.C)));
    return B.CreateSelect(Cmp, T, F);
  }
  case 25: {
    Value *Cmp = B.CreateICmpULT(V, ci32(Ctx, u32(O.A)));
    Value *T = B.CreateAdd(V, ci32(Ctx, u32(O.B)));
    Value *F = B.CreateXor(V, ci32(Ctx, u32(O.C)));
    return B.CreateSelect(Cmp, T, F);
  }
  case 27:
    return emitNarrow(B, V, 8, O, true);
  case 28:
    return emitNarrow(B, V, 16, O, true);
  case 29:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I32}),
                        {V, ci32(Ctx, u32(O.A)),
                         suppressM015FshlZeroShift(
                             B, ci32(Ctx, static_cast<uint32_t>(O.B & 31u)))});
  case 30:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I32}),
                        {ci32(Ctx, u32(O.A)), V,
                         ci32(Ctx, static_cast<uint32_t>(O.B & 31u))});
  case 31:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::uadd_sat, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 32:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::usub_sat, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 33:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sadd_sat, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 34:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ssub_sat, {I32}),
                        {V, ci32(Ctx, u32(O.A))});
  case 35:
    return emitOverflow(B, M, V, O, Intrinsic::uadd_with_overflow);
  case 36:
    return emitOverflow(B, M, V, O, Intrinsic::usub_with_overflow);
  case 37:
    return emitOverflow(B, M, V, O, Intrinsic::sadd_with_overflow);
  case 38:
    return emitOverflow(B, M, V, O, Intrinsic::ssub_with_overflow);
  case 39:
    return emitOverflow(B, M, V, O, Intrinsic::umul_with_overflow);
  case 40:
    return emitOverflow(B, M, V, O, Intrinsic::smul_with_overflow);
  case 41:
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::abs, {I32}),
                        {V, ConstantInt::getFalse(Ctx)});
  case 42: {
    Value *Shift = B.CreateAnd(V, ci32(Ctx, 31));
    Shift = suppressM015FshlZeroShift(B, Shift);
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I32}),
                        {V, ci32(Ctx, u32(O.A)), Shift});
  }
  case 43:
    return emitPackedVector(B, V, 8, O);
  case 44:
    return emitPackedVector(B, V, 16, O);
  case 45:
    return emitSmallSat(B, M, V, O, 8, Intrinsic::uadd_sat, false);
  case 46:
    return emitSmallSat(B, M, V, O, 8, Intrinsic::usub_sat, false);
  case 47:
    return emitSmallSat(B, M, V, O, 8, Intrinsic::sadd_sat, true);
  case 48:
    return emitSmallSat(B, M, V, O, 8, Intrinsic::ssub_sat, true);
  case 49:
    return emitSmallSat(B, M, V, O, 16, Intrinsic::uadd_sat, false);
  case 50:
    return emitSmallSat(B, M, V, O, 16, Intrinsic::usub_sat, false);
  case 51:
    return emitSmallSat(B, M, V, O, 16, Intrinsic::sadd_sat, true);
  case 52:
    return emitSmallSat(B, M, V, O, 16, Intrinsic::ssub_sat, true);
  case 53:
    return emitPrivateMemory(B, V, Idx, O);
  case 54:
    return emitRotate(B, M, V, O, Intrinsic::fshl);
  case 55:
    return emitRotate(B, M, V, O, Intrinsic::fshr);
  case 56: {
    Value *Cmp = B.CreateICmpEQ(B.CreateAnd(V, ci32(Ctx, u32(O.A) | 1u)),
                                ci32(Ctx, u32(O.B) & (u32(O.A) | 1u)));
    Value *T = emitPackedVector(B, V, 8, O);
    Value *F = emitSmallSat(B, M, V, O, 16, Intrinsic::uadd_sat, false);
    return B.CreateSelect(Cmp, T, F);
  }
  case 57: {
    Value *Cmp = B.CreateICmpSLT(B.CreateXor(V, ci32(Ctx, u32(O.A))),
                                 ci32(Ctx, u32(O.B)));
    Value *T = emitPackedVector(B, V, 16, O);
    Value *F = emitSmallSat(B, M, V, O, 8, Intrinsic::ssub_sat, true);
    return B.CreateSelect(Cmp, T, F);
  }
  case 58: {
    Value *Loaded = emitPrivateMemory(B, V, Idx, O);
    return emitRotate(B, M, Loaded, O, Intrinsic::fshl);
  }
  case 59:
    return emitNarrowBitIntrinsic(B, M, V, 8, Intrinsic::ctlz);
  case 60:
    return emitNarrowBitIntrinsic(B, M, V, 8, Intrinsic::cttz);
  case 61:
    return emitNarrowBitIntrinsic(B, M, V, 8, Intrinsic::ctpop);
  case 62:
    return emitNarrowBitIntrinsic(B, M, V, 8, Intrinsic::bitreverse);
  case 63:
    return emitNarrowAbs(B, M, V, 8, false);
  case 64:
    return emitNarrowBitIntrinsic(B, M, V, 16, Intrinsic::ctlz);
  case 65:
    return emitNarrowBitIntrinsic(B, M, V, 16, Intrinsic::cttz);
  case 66:
    return emitNarrowBitIntrinsic(B, M, V, 16, Intrinsic::ctpop);
  case 67:
    return emitNarrowBitIntrinsic(B, M, V, 16, Intrinsic::bitreverse);
  case 68:
    return emitNarrowBitIntrinsic(B, M, V, 16, Intrinsic::bswap);
  case 69:
    return emitNarrowAbs(B, M, V, 16, true);
  case 70:
    return emitWideOverflow(B, M, V, O, Intrinsic::uadd_with_overflow);
  case 71:
    return emitWideOverflow(B, M, V, O, Intrinsic::usub_with_overflow);
  case 72:
    return emitWideOverflow(B, M, V, O, Intrinsic::sadd_with_overflow);
  case 73:
    return emitWideOverflow(B, M, V, O, Intrinsic::ssub_with_overflow);
  case 74:
    return emitWideOverflow(B, M, V, O, Intrinsic::umul_with_overflow);
  case 75:
    return emitNarrowMinMax(B, M, V, O, 8, Intrinsic::umin, false);
  case 76:
    return emitNarrowMinMax(B, M, V, O, 8, Intrinsic::umax, false);
  case 77:
    return emitNarrowMinMax(B, M, V, O, 8, Intrinsic::smin, true);
  case 78:
    return emitNarrowMinMax(B, M, V, O, 8, Intrinsic::smax, true);
  case 79:
    return emitNarrowMinMax(B, M, V, O, 16, Intrinsic::umin, false);
  case 80:
    return emitNarrowMinMax(B, M, V, O, 16, Intrinsic::umax, false);
  case 81:
    return emitNarrowMinMax(B, M, V, O, 16, Intrinsic::smin, true);
  case 82:
    return emitNarrowMinMax(B, M, V, O, 16, Intrinsic::smax, true);
  case 83:
    return emitWideCompareSelect(B, V, O, false);
  case 84:
    return emitWideCompareSelect(B, V, O, true);
  case 85:
    return emitWideMinMax(B, M, V, O, Intrinsic::umin);
  case 86:
    return emitWideMinMax(B, M, V, O, Intrinsic::umax);
  case 87:
    return emitWideMinMax(B, M, V, O, Intrinsic::smin);
  case 88:
    return emitWideMinMax(B, M, V, O, Intrinsic::smax);
  case 89:
    return emitWideFshr(B, M, V, O, false);
  case 90:
    return emitWideFshr(B, M, V, O, true);
  case 91:
    return emitDynamicShift32(B, V, O, 0);
  case 92:
    return emitDynamicShift32(B, V, O, 1);
  case 93:
    return emitDynamicShift32(B, V, O, 2);
  case 94:
    return emitDynamicShift64(B, V, O, 0);
  case 95:
    return emitDynamicShift64(B, V, O, 1);
  case 96:
    return emitDynamicShift64(B, V, O, 2);
  case 97:
    return emitNarrowDynamicShift(B, V, O, 8, 0, false);
  case 98:
    return emitNarrowDynamicShift(B, V, O, 8, 1, false);
  case 99:
    return emitNarrowDynamicShift(B, V, O, 8, 2, true);
  case 100:
    return emitNarrowDynamicShift(B, V, O, 16, 0, false);
  case 101:
    return emitNarrowDynamicShift(B, V, O, 16, 1, false);
  case 102:
    return emitNarrowDynamicShift(B, V, O, 16, 2, true);
  case 103:
    return emitPackedVectorSat(B, M, V, 8, O, Intrinsic::uadd_sat);
  case 104:
    return emitPackedVectorSat(B, M, V, 8, O, Intrinsic::usub_sat);
  case 105:
    return emitPackedVectorSat(B, M, V, 8, O, Intrinsic::sadd_sat);
  case 106:
    return emitPackedVectorSat(B, M, V, 8, O, Intrinsic::ssub_sat);
  case 107:
    return emitPackedVectorSat(B, M, V, 16, O, Intrinsic::uadd_sat);
  case 108:
    return emitPackedVectorSat(B, M, V, 16, O, Intrinsic::usub_sat);
  case 109:
    return emitPackedVectorSat(B, M, V, 16, O, Intrinsic::sadd_sat);
  case 110:
    return emitPackedVectorSat(B, M, V, 16, O, Intrinsic::ssub_sat);
  case 111:
    return emitPackedVectorBitIntrinsic(B, M, V, 8, Intrinsic::ctlz);
  case 112:
    return emitPackedVectorBitIntrinsic(B, M, V, 8, Intrinsic::cttz);
  case 113:
    return emitPackedVectorBitIntrinsic(B, M, V, 8, Intrinsic::ctpop);
  case 114:
    return emitPackedVectorBitIntrinsic(B, M, V, 8, Intrinsic::bitreverse);
  case 115:
    return emitPackedVectorBitIntrinsic(B, M, V, 16, Intrinsic::ctpop);
  case 116:
    return emitPackedVectorBitIntrinsic(B, M, V, 16, Intrinsic::bitreverse);
  case 117:
    return emitPackedDynamicShift(B, V, 8, O);
  case 118:
    return emitPackedDynamicShift(B, V, 16, O);
  case 119:
    return emitI32VectorMinMax(B, M, V, 2, O, Intrinsic::umin);
  case 120:
    return emitI32VectorMinMax(B, M, V, 2, O, Intrinsic::umax);
  case 121:
    return emitI32VectorMinMax(B, M, V, 2, O, Intrinsic::smin);
  case 122:
    return emitI32VectorMinMax(B, M, V, 2, O, Intrinsic::smax);
  case 123:
    return emitI32VectorMinMax(B, M, V, 4, O, Intrinsic::umin);
  case 124:
    return emitI32VectorMinMax(B, M, V, 4, O, Intrinsic::umax);
  case 125:
    return emitI32VectorMinMax(B, M, V, 4, O, Intrinsic::smin);
  case 126:
    return emitI32VectorMinMax(B, M, V, 4, O, Intrinsic::smax);
  case 127:
    return emitI32VectorBitIntrinsic(B, M, V, 2, O, Intrinsic::ctlz);
  case 128:
    return emitI32VectorBitIntrinsic(B, M, V, 4, O, Intrinsic::ctlz);
  case 129:
    return emitI32VectorBitIntrinsic(B, M, V, 2, O, Intrinsic::cttz);
  case 130:
    return emitI32VectorBitIntrinsic(B, M, V, 4, O, Intrinsic::cttz);
  case 131:
    return emitI32VectorBitIntrinsic(B, M, V, 2, O, Intrinsic::ctpop);
  case 132:
    return emitI32VectorBitIntrinsic(B, M, V, 4, O, Intrinsic::ctpop);
  case 133:
    return emitI32VectorDynamicShift(B, V, 2, O);
  case 134:
    return emitI32VectorDynamicShift(B, V, 4, O);
  case 135:
    return emitPackedFshr(B, M, V, 8, O, false);
  case 136:
    return emitPackedFshr(B, M, V, 16, O, false);
  case 137:
    return emitPackedFshr(B, M, V, 8, O, true);
  case 138:
    return emitPackedFshr(B, M, V, 16, O, true);
  case 139:
    return emitI32VectorFshr(B, M, V, 2, O, false);
  case 140:
    return emitI32VectorFshr(B, M, V, 4, O, false);
  case 141:
    return emitI32VectorFshr(B, M, V, 2, O, true);
  case 142:
    return emitI32VectorFshr(B, M, V, 4, O, true);
  case 143:
    return emitDerivedI32VectorMix(B, V, 2, O);
  case 144:
    return emitDerivedI32VectorMix(B, V, 4, O);
  case 145:
    return emitDerivedI32VectorMinMax(B, M, V, 2, O, Intrinsic::umin);
  case 146:
    return emitDerivedI32VectorMinMax(B, M, V, 2, O, Intrinsic::umax);
  case 147:
    return emitDerivedI32VectorMinMax(B, M, V, 2, O, Intrinsic::smin);
  case 148:
    return emitDerivedI32VectorMinMax(B, M, V, 2, O, Intrinsic::smax);
  case 149:
    return emitDerivedI32VectorMinMax(B, M, V, 4, O, Intrinsic::umin);
  case 150:
    return emitDerivedI32VectorMinMax(B, M, V, 4, O, Intrinsic::umax);
  case 151:
    return emitDerivedI32VectorMinMax(B, M, V, 4, O, Intrinsic::smin);
  case 152:
    return emitDerivedI32VectorMinMax(B, M, V, 4, O, Intrinsic::smax);
  case 153:
    return emitDerivedI32VectorUnaryIntrinsic(B, M, V, 2, O,
                                              Intrinsic::bitreverse);
  case 154:
    return emitDerivedI32VectorUnaryIntrinsic(B, M, V, 4, O,
                                              Intrinsic::bitreverse);
  case 155:
    return emitDerivedI32VectorUnaryIntrinsic(B, M, V, 2, O, Intrinsic::bswap);
  case 156:
    return emitDerivedI32VectorUnaryIntrinsic(B, M, V, 4, O, Intrinsic::bswap);
  case 157:
    return emitDerivedI32VectorUnaryIntrinsic(B, M, V, 2, O, Intrinsic::abs);
  case 158:
    return emitDerivedI32VectorUnaryIntrinsic(B, M, V, 4, O, Intrinsic::abs);
  case 159:
    return emitF32ScalarMix(B, M, V, O, false);
  case 160:
    return emitF32ScalarMix(B, M, V, O, true);
  case 161:
    return emitF64ScalarMix(B, M, V, O, false);
  case 162:
    return emitF64ScalarMix(B, M, V, O, true);
  case 163:
    return emitF32VectorMix(B, M, V, 2, O, false);
  case 164:
    return emitF32VectorMix(B, M, V, 4, O, false);
  case 165:
    return emitF32VectorMix(B, M, V, 2, O, true);
  case 166:
    return emitF32VectorMix(B, M, V, 4, O, true);
  case 167:
    return emitF16ScalarMix(B, M, V, O, false);
  case 168:
    return emitF16ScalarMix(B, M, V, O, true);
  case 169:
    return emitF16VectorMix(B, M, V, 2, O, false);
  case 170:
    return emitF16VectorMix(B, M, V, 4, O, false);
  case 171:
    return emitF16VectorMix(B, M, V, 2, O, true);
  case 172:
    return emitF16VectorMix(B, M, V, 4, O, true);
  case 173:
    return emitLocalMemory(B, M, V, Idx, O, 32);
  case 174:
    return emitLocalMemory(B, M, V, Idx, O, 16);
  case 175:
    return emitLocalMemory(B, M, V, Idx, O, 16, true);
  case 176:
    return emitLocalMemory(B, M, V, Idx, O, 8);
  case 177:
    return emitLocalMemory(B, M, V, Idx, O, 8, true);
  case 178:
    return emitLocalMemoryPair(B, M, V, Idx, O);
  case 179:
    return emitGlobalMemory(B, Scratch, V, Idx, O, 32);
  case 180:
    return emitGlobalMemory(B, Scratch, V, Idx, O, 16);
  case 181:
    return emitGlobalMemory(B, Scratch, V, Idx, O, 16, true);
  case 182:
    return emitGlobalMemory(B, Scratch, V, Idx, O, 8);
  case 183:
    return emitGlobalMemory(B, Scratch, V, Idx, O, 8, true);
  case 184:
    return emitGlobalMemoryPair(B, Scratch, V, Idx, O);
  case 185:
  case 186:
  case 187:
  case 188:
  case 189:
  case 190:
  case 191:
  case 192:
  case 193:
  case 194:
    return emitLocalAtomicRMW(B, M, V, Idx, O, O.Kind - 185);
  case 195:
  case 196:
  case 197:
  case 198:
  case 199:
  case 200:
  case 201:
  case 202:
  case 203:
  case 204:
    return emitGlobalAtomicRMW(B, Scratch, V, Idx, O, O.Kind - 195);
  default: {
    Value *Cmp = B.CreateICmpSLT(V, ci32(Ctx, u32(O.A)));
    Value *T = B.CreateXor(V, ci32(Ctx, u32(O.B)));
    Value *F = B.CreateSub(V, ci32(Ctx, u32(O.C)));
    return B.CreateSelect(Cmp, T, F);
  }
  }
}

Value *emitOps(IRBuilder<> &B, Module &M, Value *V, Value *Idx, Value *Scratch,
               ArrayRef<Op> Ops) {
  for (const Op &O : Ops)
    V = emitOp(B, M, V, Idx, Scratch, O);
  return V;
}

Value *emitStructuredOps(IRBuilder<> &B, Module &M, Function *F,
                         const Program &P, Value *V, Value *Idx,
                         Value *Scratch) {
  if (!P.UseStructuredCFG || P.Ops.size() < 4)
    return emitOps(B, M, V, Idx, Scratch, P.Ops);

  LLVMContext &Ctx = B.getContext();
  size_t Prefix = 0;
  size_t ThenLen = 0;
  size_t ElseLen = 0;
  size_t SuffixStart = 0;
  chooseStructuredSlices(P, Prefix, ThenLen, ElseLen, SuffixStart);

  V = emitOps(B, M, V, Idx, Scratch, ArrayRef<Op>(P.Ops).take_front(Prefix));
  Value *PredBase = B.CreateXor(V, Idx);
  uint32_t PredicateConst = u32(P.CFGPredicate);
  Value *Cond = nullptr;
  switch ((P.CFGPredicate >> 32) & 3u) {
  case 0:
    Cond = B.CreateICmpULT(PredBase, ci32(Ctx, PredicateConst));
    break;
  case 1:
    Cond = B.CreateICmpSLT(PredBase, ci32(Ctx, PredicateConst));
    break;
  case 2: {
    uint32_t Mask = u32(P.CFGPredicate >> 8) | 1u;
    Cond = B.CreateICmpEQ(B.CreateAnd(PredBase, ci32(Ctx, Mask)),
                          ci32(Ctx, PredicateConst & Mask));
    break;
  }
  default: {
    uint32_t Mask = u32(P.CFGPredicate >> 16) | 1u;
    Cond = B.CreateICmpNE(B.CreateAnd(PredBase, ci32(Ctx, Mask)),
                          ci32(Ctx, PredicateConst & Mask));
    break;
  }
  }

  BasicBlock *ThenBB = BasicBlock::Create(Ctx, "cfg.then", F);
  BasicBlock *ElseBB = BasicBlock::Create(Ctx, "cfg.else", F);
  BasicBlock *MergeBB = BasicBlock::Create(Ctx, "cfg.merge", F);
  B.CreateCondBr(Cond, ThenBB, ElseBB);

  ArrayRef<Op> Ops(P.Ops);
  B.SetInsertPoint(ThenBB);
  Value *ThenV =
      emitOps(B, M, V, Idx, Scratch, Ops.slice(Prefix, ThenLen));
  B.CreateBr(MergeBB);
  ThenBB = B.GetInsertBlock();

  B.SetInsertPoint(ElseBB);
  Value *ElseV =
      emitOps(B, M, V, Idx, Scratch, Ops.slice(Prefix + ThenLen, ElseLen));
  B.CreateBr(MergeBB);
  ElseBB = B.GetInsertBlock();

  B.SetInsertPoint(MergeBB);
  PHINode *Phi = B.CreatePHI(Type::getInt32Ty(Ctx), 2);
  Phi->addIncoming(ThenV, ThenBB);
  Phi->addIncoming(ElseV, ElseBB);
  return emitOps(B, M, Phi, Idx, Scratch, Ops.drop_front(SuffixStart));
}

std::unique_ptr<Module> buildModule(LLVMContext &Ctx, const Program &P,
                                    StringRef CPU, StringRef KernelName) {
  auto M = std::make_unique<Module>("fuzzx_amdgpu_diff", Ctx);
  M->setTargetTriple(Triple("amdgcn-amd-amdhsa"));
  M->setDataLayout(DataLayout);
  M->addModuleFlag(Module::Error, "amdhsa_code_object_version", 600);
  M->addModuleFlag(Module::Error, "amdgpu_printf_kind",
                   MDString::get(Ctx, "hostcall"));
  M->addModuleFlag(Module::Max, "PIC Level", 2);

  Type *VoidTy = Type::getVoidTy(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *GlobalPtr = PointerType::get(Ctx, 1);
  auto *FTy = FunctionType::get(VoidTy, {GlobalPtr, GlobalPtr, I32}, false);
  Function *F =
      Function::Create(FTy, GlobalValue::ExternalLinkage, KernelName, *M);
  F->setCallingConv(CallingConv::AMDGPU_KERNEL);
  F->setVisibility(GlobalValue::ProtectedVisibility);
  F->addFnAttr(Attribute::Convergent);
  F->addFnAttr(Attribute::NoUnwind);
  F->addFnAttr("amdgpu-flat-work-group-size", "1,256");
  F->addFnAttr("target-cpu", CPU);
  F->addFnAttr("uniform-work-group-size", "true");

  auto ArgIt = F->arg_begin();
  Argument *In = ArgIt++;
  Argument *Out = ArgIt++;
  Argument *N = ArgIt++;
  In->setName("in");
  Out->setName("out");
  N->setName("n");

  BasicBlock *Entry = BasicBlock::Create(Ctx, "entry", F);
  BasicBlock *Body = BasicBlock::Create(Ctx, "body", F);
  BasicBlock *Exit = BasicBlock::Create(Ctx, "exit", F);
  IRBuilder<> B(Entry);

  Function *Workgroup =
      Intrinsic::getOrInsertDeclaration(M.get(), Intrinsic::amdgcn_workgroup_id_x);
  Function *Workitem =
      Intrinsic::getOrInsertDeclaration(M.get(), Intrinsic::amdgcn_workitem_id_x);
  Value *WG = B.CreateCall(Workgroup);
  Value *WI = B.CreateCall(Workitem);
  Value *Idx = B.CreateAdd(B.CreateMul(WG, ci32(Ctx, ThreadsPerBlock)), WI);
  Value *Ok = B.CreateICmpULT(Idx, N);
  B.CreateCondBr(Ok, Body, Exit);

  B.SetInsertPoint(Body);
  Value *Idx64 = B.CreateZExt(Idx, I64);
  Value *InPtr = B.CreateGEP(I32, In, Idx64);
  Value *V = B.CreateLoad(I32, InPtr);
  V = emitStructuredOps(B, *M, F, P, V, Idx, Out);
  Value *OutPtr = B.CreateGEP(I32, Out, Idx64);
  B.CreateStore(V, OutPtr);
  B.CreateBr(Exit);

  B.SetInsertPoint(Exit);
  B.CreateRetVoid();
  return M;
}

const Target *getAMDGPUTarget() {
  static const Target *T = [] {
    LLVMInitializeAMDGPUTargetInfo();
    LLVMInitializeAMDGPUTarget();
    LLVMInitializeAMDGPUTargetMC();
    LLVMInitializeAMDGPUAsmPrinter();
    LLVMInitializeAMDGPUAsmParser();

    std::string Error;
    Triple TT("amdgcn-amd-amdhsa");
    const Target *Target = TargetRegistry::lookupTarget(TT, Error);
    if (!Target)
      std::abort();
    return Target;
  }();
  return T;
}

CodeGenOptLevel codeGenOptLevel(OptimizationLevel Level) {
  if (Level == OptimizationLevel::O0)
    return CodeGenOptLevel::None;
  if (Level == OptimizationLevel::O1)
    return CodeGenOptLevel::Less;
  if (Level == OptimizationLevel::O2)
    return CodeGenOptLevel::Default;
  return CodeGenOptLevel::Aggressive;
}

TargetMachine *getTargetMachine(StringRef CPU, OptimizationLevel Level) {
  static std::unique_ptr<TargetMachine> O0TM;
  static std::unique_ptr<TargetMachine> O2TM;
  std::unique_ptr<TargetMachine> &TM =
      Level == OptimizationLevel::O0 ? O0TM : O2TM;
  if (TM)
    return TM.get();

  Triple TT("amdgcn-amd-amdhsa");
  TargetOptions Options;
  TM.reset(getAMDGPUTarget()->createTargetMachine(
      TT, CPU, "", Options, Reloc::PIC_, std::nullopt,
      codeGenOptLevel(Level)));
  if (!TM)
    std::abort();
  return TM.get();
}

bool runOptimizationPipeline(Module &M, TargetMachine &TM,
                             OptimizationLevel Level) {
  LoopAnalysisManager LAM;
  FunctionAnalysisManager FAM;
  CGSCCAnalysisManager CGAM;
  ModuleAnalysisManager MAM;

  PassBuilder PB(&TM);
  PB.registerModuleAnalyses(MAM);
  PB.registerCGSCCAnalyses(CGAM);
  PB.registerFunctionAnalyses(FAM);
  PB.registerLoopAnalyses(LAM);
  PB.crossRegisterProxies(LAM, FAM, CGAM, MAM);

  ModulePassManager MPM =
      Level == OptimizationLevel::O0 ? PB.buildO0DefaultPipeline(Level)
                                     : PB.buildPerModuleDefaultPipeline(Level);
  MPM.run(M, MAM);
  return !verifyModule(M, &errs());
}

std::string tempPath(StringRef Suffix) {
  static std::atomic<unsigned> Counter{0};
  auto Dir = std::filesystem::temp_directory_path();
  auto Now = std::chrono::steady_clock::now().time_since_epoch().count();
  return (Dir / ("fuzzx-amdgpu-diff-" + std::to_string(getpid()) + "-" +
                 std::to_string(Now) + "-" + std::to_string(Counter++) +
                 Suffix.str()))
      .string();
}

bool writeBytes(StringRef Path, ArrayRef<char> Bytes) {
  std::ofstream Out(Path.str(), std::ios::binary);
  if (!Out)
    return false;
  Out.write(Bytes.data(), static_cast<std::streamsize>(Bytes.size()));
  return static_cast<bool>(Out);
}

std::optional<std::string> linkObjectsToHsaco(ArrayRef<char> O0Obj,
                                              ArrayRef<char> O2Obj) {
  std::string O0ObjPath = tempPath("-o0.o");
  std::string O2ObjPath = tempPath("-o2.o");
  std::string HsacoPath = tempPath(".hsaco");
  if (!writeBytes(O0ObjPath, O0Obj) || !writeBytes(O2ObjPath, O2Obj)) {
    std::filesystem::remove(O0ObjPath);
    std::filesystem::remove(O2ObjPath);
    return std::nullopt;
  }

  std::vector<const char *> Args = {"ld.lld", "-shared", O0ObjPath.c_str(),
                                    O2ObjPath.c_str(), "-o",
                                    HsacoPath.c_str()};
  std::string StdoutText;
  std::string StderrText;
  raw_string_ostream StdoutOS(StdoutText);
  raw_string_ostream StderrOS(StderrText);
  bool Ok = lld::elf::link(Args, StdoutOS, StderrOS, false, false);
  std::filesystem::remove(O0ObjPath);
  std::filesystem::remove(O2ObjPath);
  if (!Ok) {
    std::filesystem::remove(HsacoPath);
    return std::nullopt;
  }
  return HsacoPath;
}

std::optional<SmallVector<char, 0>> emitObject(Module &M, TargetMachine &TM) {
  M.setDataLayout(TM.createDataLayout());
  if (verifyModule(M, &errs()))
    return std::nullopt;

  SmallVector<char, 0> Obj;
  raw_svector_ostream OS(Obj);
  legacy::PassManager PM;
  if (TM.addPassesToEmitFile(PM, OS, nullptr, CodeGenFileType::ObjectFile))
    return std::nullopt;
  PM.run(M);
  return Obj;
}

std::string moduleToString(Module &M) {
  std::string Text;
  raw_string_ostream OS(Text);
  M.print(OS, nullptr);
  return Text;
}

std::optional<SmallVector<char, 0>>
compileProgramToObject(const Program &P, StringRef CPU, OptimizationLevel Level,
                       StringRef KernelName, std::string *IR = nullptr) {
  TargetMachine *TM = getTargetMachine(CPU, Level);
  LLVMContext Ctx;
  std::unique_ptr<Module> M = buildModule(Ctx, P, CPU, KernelName);
  M->setDataLayout(TM->createDataLayout());
  if (IR)
    *IR = moduleToString(*M);
  if (!runOptimizationPipeline(*M, *TM, Level))
    return std::nullopt;
  return emitObject(*M, *TM);
}

struct HipBuffers {
  uint32_t *In = nullptr;
  uint32_t *O0Out = nullptr;
  uint32_t *O2Out = nullptr;
  size_t Capacity = 0;
};

HipBuffers &hipBuffers() {
  static HipBuffers Buffers;
  return Buffers;
}

bool ensureHipBuffers(size_t Count) {
  HipBuffers &Buffers = hipBuffers();
  if (Buffers.Capacity >= Count)
    return true;
  if (Buffers.In)
    (void)hipFree(Buffers.In);
  if (Buffers.O0Out)
    (void)hipFree(Buffers.O0Out);
  if (Buffers.O2Out)
    (void)hipFree(Buffers.O2Out);
  Buffers = {};
  if (hipMalloc(&Buffers.In, Count * sizeof(uint32_t)) != hipSuccess)
    return false;
  if (hipMalloc(&Buffers.O0Out, Count * sizeof(uint32_t)) != hipSuccess) {
    (void)hipFree(Buffers.In);
    Buffers = {};
    return false;
  }
  if (hipMalloc(&Buffers.O2Out, Count * sizeof(uint32_t)) != hipSuccess) {
    (void)hipFree(Buffers.In);
    (void)hipFree(Buffers.O0Out);
    Buffers = {};
    return false;
  }
  Buffers.Capacity = Count;
  return true;
}

bool launchKernel(hipFunction_t Kernel, uint32_t *Out, size_t Count) {
  HipBuffers &Buffers = hipBuffers();
  if (hipMemset(Out, 0, Count * sizeof(uint32_t)) != hipSuccess)
    return false;

  uint32_t N = static_cast<uint32_t>(Count);
  void *Args[] = {&Buffers.In, &Out, &N};
  unsigned Blocks = (N + ThreadsPerBlock - 1) / ThreadsPerBlock;
  return hipModuleLaunchKernel(Kernel, Blocks, 1, 1, ThreadsPerBlock, 1, 1, 0,
                               nullptr, Args, nullptr) == hipSuccess;
}

bool runBothOnGpu(const std::string &HsacoPath, ArrayRef<uint32_t> Inputs,
                  MutableArrayRef<uint32_t> O0Outputs,
                  MutableArrayRef<uint32_t> O2Outputs) {
  if (!ensureHipBuffers(Inputs.size()))
    return false;

  HipBuffers &Buffers = hipBuffers();
  if (hipMemcpy(Buffers.In, Inputs.data(), Inputs.size() * sizeof(uint32_t),
                hipMemcpyHostToDevice) != hipSuccess)
    return false;

  hipModule_t Module = nullptr;
  hipFunction_t O0Kernel = nullptr;
  hipFunction_t O2Kernel = nullptr;
  if (hipModuleLoad(&Module, HsacoPath.c_str()) != hipSuccess)
    return false;
  if (hipModuleGetFunction(&O0Kernel, Module, "fuzz_kernel_o0") != hipSuccess ||
      hipModuleGetFunction(&O2Kernel, Module, "fuzz_kernel_o2") != hipSuccess) {
    (void)hipModuleUnload(Module);
    return false;
  }

  bool Ok = launchKernel(O0Kernel, Buffers.O0Out, O0Outputs.size()) &&
            launchKernel(O2Kernel, Buffers.O2Out, O2Outputs.size()) &&
            hipDeviceSynchronize() == hipSuccess &&
            hipMemcpy(O0Outputs.data(), Buffers.O0Out,
                      O0Outputs.size() * sizeof(uint32_t),
                      hipMemcpyDeviceToHost) == hipSuccess &&
            hipMemcpy(O2Outputs.data(), Buffers.O2Out,
                      O2Outputs.size() * sizeof(uint32_t),
                      hipMemcpyDeviceToHost) == hipSuccess;
  (void)hipModuleUnload(Module);
  return Ok;
}

void saveFinding(const uint8_t *Data, size_t Size, StringRef IR,
                 const std::string &HsacoPath, StringRef Kind, unsigned Index,
                 uint32_t Input, uint32_t O0Value, uint32_t O2Value,
                 std::optional<uint32_t> Expected = std::nullopt) {
  const char *RootEnv = std::getenv("FUZZX_FINDINGS_DIR");
  std::filesystem::path Root = RootEnv && *RootEnv ? RootEnv : "findings";
  std::filesystem::create_directories(Root);
  auto Dir = Root / ("cxx-diff-" + std::to_string(std::time(nullptr)) + "-" +
                     std::to_string(getpid()));
  std::filesystem::create_directories(Dir);

  std::ofstream Raw(Dir / "fuzzer-input.bin", std::ios::binary);
  Raw.write(reinterpret_cast<const char *>(Data),
            static_cast<std::streamsize>(Size));
  std::ofstream LL(Dir / "program.ll");
  LL << IR.str();
  std::filesystem::copy_file(HsacoPath, Dir / "program.hsaco",
                             std::filesystem::copy_options::overwrite_existing);
  std::ofstream Mismatch(Dir / "mismatch.txt");
  Mismatch << "kind=" << Kind.str() << "\n"
           << "index=" << Index << "\n"
           << "input=0x" << utohexstr(Input) << "\n"
           << "o0=0x" << utohexstr(O0Value) << "\n"
           << "o2=0x" << utohexstr(O2Value) << "\n";
  if (Expected)
    Mismatch << "expected=0x" << utohexstr(*Expected) << "\n";
  errs() << "candidate saved: " << Dir.string() << "\n";
}

StringRef getCPU() {
  const char *CPU = std::getenv("AMDGPU_MCPU");
  return CPU && *CPU ? StringRef(CPU) : StringRef("gfx950");
}

int getDevice() {
  const char *Device = std::getenv("HIP_DEVICE");
  return Device && *Device ? std::atoi(Device) : 0;
}

} // namespace

extern "C" int LLVMFuzzerTestOneInput(const uint8_t *Data, size_t Size) {
  if (Size < 2 || Size > 4096)
    return 0;

  StringRef CPU = getCPU();
  Program P = makeProgram(Data, Size);
  auto Inputs = makeInputs(Data, Size);

  std::string IR;
  auto O0Obj = compileProgramToObject(P, CPU, OptimizationLevel::O0,
                                      "fuzz_kernel_o0", &IR);
  if (!O0Obj)
    return 0;
  auto O2Obj =
      compileProgramToObject(P, CPU, OptimizationLevel::O2, "fuzz_kernel_o2");
  if (!O2Obj)
    return 0;
  auto HsacoPath = linkObjectsToHsaco(*O0Obj, *O2Obj);
  if (!HsacoPath)
    return 0;

  std::array<uint32_t, InputCount> O0Outputs{};
  std::array<uint32_t, InputCount> O2Outputs{};
  bool Ran =
      runBothOnGpu(*HsacoPath, Inputs, MutableArrayRef<uint32_t>(O0Outputs),
                   MutableArrayRef<uint32_t>(O2Outputs));
  if (!Ran) {
    std::filesystem::remove(*HsacoPath);
    return 0;
  }

  std::optional<std::array<uint32_t, InputCount>> Expected;
  if (oracleEnabled())
    Expected = evalProgramForInputs(P, ArrayRef<uint32_t>(Inputs));

  for (unsigned I = 0; I < InputCount; ++I) {
    if (Expected &&
        (O0Outputs[I] != (*Expected)[I] || O2Outputs[I] != (*Expected)[I])) {
      StringRef Kind = O0Outputs[I] == O2Outputs[I] ? "oracle-shared"
                       : O0Outputs[I] == (*Expected)[I] ? "oracle-o2"
                       : O2Outputs[I] == (*Expected)[I] ? "oracle-o0"
                                                        : "oracle-both";
      saveFinding(Data, Size, IR, *HsacoPath, Kind, I, Inputs[I],
                  O0Outputs[I], O2Outputs[I], (*Expected)[I]);
      std::abort();
    }
    if (O0Outputs[I] != O2Outputs[I]) {
      saveFinding(Data, Size, IR, *HsacoPath, "differential", I, Inputs[I],
                  O0Outputs[I], O2Outputs[I]);
      std::abort();
    }
  }
  std::filesystem::remove(*HsacoPath);
  return 0;
}

extern "C" int LLVMFuzzerInitialize(int *, char ***) {
  if (hipSetDevice(getDevice()) != hipSuccess)
    return 1;
  return 0;
}
