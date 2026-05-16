#include "lld/Common/Driver.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/ADT/StringExtras.h"
#include "llvm/IR/Constants.h"
#include "llvm/IR/Function.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/Intrinsics.h"
#include "llvm/IR/IntrinsicsAMDGPU.h"
#include "llvm/IR/LegacyPassManager.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/Verifier.h"
#include "llvm/MC/TargetRegistry.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Support/CodeGen.h"
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

bool hasFiveShlAddPairs(ArrayRef<Op> Ops) {
  unsigned Pairs = 0;
  bool NeedAdd = false;
  for (const Op &O : Ops) {
    if (NeedAdd) {
      if (isI32Add(O)) {
        ++Pairs;
        NeedAdd = false;
        if (Pairs >= 5)
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
  bool AllowM004 = envFlag("FUZZX_ALLOW_M004_VECTOR_IDENTITY_XOR", false) ||
                   envFlag("FUZZX_ALLOW_M007_VECTOR_IDENTITY_XOR", false);
  P.Ops.reserve(OpCount);
  for (unsigned I = 0; I < OpCount; ++I) {
    uint8_t RawKind = BS.next8();
    uint8_t Kind = RawKind % 27;
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
    if (!AllowM003) {
      P.Ops.push_back(O);
      if (hasFiveShlAddPairs(P.Ops)) {
        P.Ops.back().Kind = 3;
        ++P.Ops.back().A;
      } else if (hasAddShlLadder(P.Ops)) {
        P.Ops.back().Kind = 11;
      }
      continue;
    }
    P.Ops.push_back(O);
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

ConstantInt *ci32(LLVMContext &Ctx, uint32_t V) {
  return ConstantInt::get(Type::getInt32Ty(Ctx), V);
}

ConstantInt *ci64(LLVMContext &Ctx, uint64_t V) {
  return ConstantInt::get(Type::getInt64Ty(Ctx), V);
}

Constant *vectorConstant(LLVMContext &Ctx, ArrayRef<uint32_t> Values,
                         bool ShiftAmounts = false) {
  SmallVector<Constant *, 4> Constants;
  for (uint32_t V : Values)
    Constants.push_back(ci32(Ctx, ShiftAmounts ? (V & 31u) : V));
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

Value *emitRotate(IRBuilder<> &B, Module &M, Value *V, const Op &O,
                  Intrinsic::ID ID) {
  LLVMContext &Ctx = B.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Shift = B.CreateAnd(B.CreateXor(V, ci32(Ctx, u32(O.B))), ci32(Ctx, 31));
  return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {I32}),
                      {V, ci32(Ctx, u32(O.A)), Shift});
}

Value *emitOp(IRBuilder<> &B, Module &M, Value *V, Value *Idx, const Op &O) {
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
                         ci32(Ctx, static_cast<uint32_t>(O.B & 31u))});
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
  default: {
    Value *Cmp = B.CreateICmpSLT(V, ci32(Ctx, u32(O.A)));
    Value *T = B.CreateXor(V, ci32(Ctx, u32(O.B)));
    Value *F = B.CreateSub(V, ci32(Ctx, u32(O.C)));
    return B.CreateSelect(Cmp, T, F);
  }
  }
}

Value *emitOps(IRBuilder<> &B, Module &M, Value *V, Value *Idx,
               ArrayRef<Op> Ops) {
  for (const Op &O : Ops)
    V = emitOp(B, M, V, Idx, O);
  return V;
}

Value *emitStructuredOps(IRBuilder<> &B, Module &M, Function *F,
                         const Program &P, Value *V, Value *Idx) {
  if (!P.UseStructuredCFG || P.Ops.size() < 4)
    return emitOps(B, M, V, Idx, P.Ops);

  LLVMContext &Ctx = B.getContext();
  size_t Prefix = 0;
  size_t ThenLen = 0;
  size_t ElseLen = 0;
  size_t SuffixStart = 0;
  chooseStructuredSlices(P, Prefix, ThenLen, ElseLen, SuffixStart);

  V = emitOps(B, M, V, Idx, ArrayRef<Op>(P.Ops).take_front(Prefix));
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
  Value *ThenV = emitOps(B, M, V, Idx, Ops.slice(Prefix, ThenLen));
  B.CreateBr(MergeBB);
  ThenBB = B.GetInsertBlock();

  B.SetInsertPoint(ElseBB);
  Value *ElseV = emitOps(B, M, V, Idx, Ops.slice(Prefix + ThenLen, ElseLen));
  B.CreateBr(MergeBB);
  ElseBB = B.GetInsertBlock();

  B.SetInsertPoint(MergeBB);
  PHINode *Phi = B.CreatePHI(Type::getInt32Ty(Ctx), 2);
  Phi->addIncoming(ThenV, ThenBB);
  Phi->addIncoming(ElseV, ElseBB);
  return emitOps(B, M, Phi, Idx, Ops.drop_front(SuffixStart));
}

std::unique_ptr<Module> buildModule(LLVMContext &Ctx, const Program &P,
                                    StringRef CPU) {
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
      Function::Create(FTy, GlobalValue::ExternalLinkage, "fuzz_kernel", *M);
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
  V = emitStructuredOps(B, *M, F, P, V, Idx);
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

std::optional<std::string> linkObjectToHsaco(ArrayRef<char> Obj) {
  std::string ObjPath = tempPath(".o");
  std::string HsacoPath = tempPath(".hsaco");
  if (!writeBytes(ObjPath, Obj))
    return std::nullopt;

  std::vector<const char *> Args = {"ld.lld", "-shared", ObjPath.c_str(),
                                    "-o", HsacoPath.c_str()};
  std::string StdoutText;
  std::string StderrText;
  raw_string_ostream StdoutOS(StdoutText);
  raw_string_ostream StderrOS(StderrText);
  bool Ok = lld::elf::link(Args, StdoutOS, StderrOS, false, false);
  std::filesystem::remove(ObjPath);
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

std::optional<std::string> compileProgramToHsaco(const Program &P,
                                                 StringRef CPU,
                                                 OptimizationLevel Level,
                                                 std::string *IR = nullptr) {
  TargetMachine *TM = getTargetMachine(CPU, Level);
  LLVMContext Ctx;
  std::unique_ptr<Module> M = buildModule(Ctx, P, CPU);
  M->setDataLayout(TM->createDataLayout());
  if (IR)
    *IR = moduleToString(*M);
  if (!runOptimizationPipeline(*M, *TM, Level))
    return std::nullopt;
  auto Obj = emitObject(*M, *TM);
  if (!Obj)
    return std::nullopt;
  return linkObjectToHsaco(*Obj);
}

struct HipBuffers {
  uint32_t *In = nullptr;
  uint32_t *Out = nullptr;
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
  if (Buffers.Out)
    (void)hipFree(Buffers.Out);
  Buffers = {};
  if (hipMalloc(&Buffers.In, Count * sizeof(uint32_t)) != hipSuccess)
    return false;
  if (hipMalloc(&Buffers.Out, Count * sizeof(uint32_t)) != hipSuccess) {
    (void)hipFree(Buffers.In);
    Buffers = {};
    return false;
  }
  Buffers.Capacity = Count;
  return true;
}

bool runOnGpu(const std::string &HsacoPath, ArrayRef<uint32_t> Inputs,
              MutableArrayRef<uint32_t> Outputs) {
  if (!ensureHipBuffers(Inputs.size()))
    return false;

  HipBuffers &Buffers = hipBuffers();
  if (hipMemcpy(Buffers.In, Inputs.data(), Inputs.size() * sizeof(uint32_t),
                hipMemcpyHostToDevice) != hipSuccess)
    return false;
  if (hipMemset(Buffers.Out, 0, Outputs.size() * sizeof(uint32_t)) != hipSuccess)
    return false;

  hipModule_t Module = nullptr;
  hipFunction_t Kernel = nullptr;
  if (hipModuleLoad(&Module, HsacoPath.c_str()) != hipSuccess)
    return false;
  if (hipModuleGetFunction(&Kernel, Module, "fuzz_kernel") != hipSuccess) {
    (void)hipModuleUnload(Module);
    return false;
  }

  uint32_t N = static_cast<uint32_t>(Outputs.size());
  void *Args[] = {&Buffers.In, &Buffers.Out, &N};
  unsigned Blocks = (N + ThreadsPerBlock - 1) / ThreadsPerBlock;
  bool Ok = hipModuleLaunchKernel(Kernel, Blocks, 1, 1, ThreadsPerBlock, 1, 1,
                                  0, nullptr, Args, nullptr) == hipSuccess &&
            hipDeviceSynchronize() == hipSuccess &&
            hipMemcpy(Outputs.data(), Buffers.Out,
                      Outputs.size() * sizeof(uint32_t),
                      hipMemcpyDeviceToHost) == hipSuccess;
  (void)hipModuleUnload(Module);
  return Ok;
}

void saveFinding(const uint8_t *Data, size_t Size, StringRef IR,
                 const std::string &O0HsacoPath,
                 const std::string &O2HsacoPath, unsigned Index,
                 uint32_t Input, uint32_t O0Value, uint32_t O2Value) {
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
  std::filesystem::copy_file(O0HsacoPath, Dir / "program-O0.hsaco",
                             std::filesystem::copy_options::overwrite_existing);
  std::filesystem::copy_file(O2HsacoPath, Dir / "program-O2.hsaco",
                             std::filesystem::copy_options::overwrite_existing);
  std::ofstream Mismatch(Dir / "mismatch.txt");
  Mismatch << "index=" << Index << "\n"
           << "input=0x" << utohexstr(Input) << "\n"
           << "o0=0x" << utohexstr(O0Value) << "\n"
           << "o2=0x" << utohexstr(O2Value) << "\n";
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
  auto O0HsacoPath =
      compileProgramToHsaco(P, CPU, OptimizationLevel::O0, &IR);
  if (!O0HsacoPath)
    return 0;
  auto O2HsacoPath = compileProgramToHsaco(P, CPU, OptimizationLevel::O2);
  if (!O2HsacoPath) {
    std::filesystem::remove(*O0HsacoPath);
    return 0;
  }

  std::array<uint32_t, InputCount> O0Outputs{};
  std::array<uint32_t, InputCount> O2Outputs{};
  bool RanO0 =
      runOnGpu(*O0HsacoPath, Inputs, MutableArrayRef<uint32_t>(O0Outputs));
  bool RanO2 =
      runOnGpu(*O2HsacoPath, Inputs, MutableArrayRef<uint32_t>(O2Outputs));
  if (!RanO0 || !RanO2) {
    std::filesystem::remove(*O0HsacoPath);
    std::filesystem::remove(*O2HsacoPath);
    return 0;
  }

  for (unsigned I = 0; I < InputCount; ++I) {
    if (O0Outputs[I] != O2Outputs[I]) {
      saveFinding(Data, Size, IR, *O0HsacoPath, *O2HsacoPath, I, Inputs[I],
                  O0Outputs[I], O2Outputs[I]);
      std::abort();
    }
  }
  std::filesystem::remove(*O0HsacoPath);
  std::filesystem::remove(*O2HsacoPath);
  return 0;
}

extern "C" int LLVMFuzzerInitialize(int *, char ***) {
  if (hipSetDevice(getDevice()) != hipSuccess)
    return 1;
  return 0;
}
