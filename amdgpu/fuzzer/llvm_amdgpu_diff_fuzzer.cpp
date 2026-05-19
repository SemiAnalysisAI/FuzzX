#include "lld/Common/Driver.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/SmallPtrSet.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/ADT/StringExtras.h"
#include "llvm/Bitcode/BitcodeReader.h"
#include "llvm/Bitcode/BitcodeWriter.h"
#include "llvm/ExecutionEngine/ExecutionEngine.h"
#include "llvm/ExecutionEngine/GenericValue.h"
#include "llvm/ExecutionEngine/Interpreter.h"
#include "llvm/IR/Constants.h"
#include "llvm/IR/CFG.h"
#include "llvm/IR/DerivedTypes.h"
#include "llvm/IR/Dominators.h"
#include "llvm/IR/Function.h"
#include "llvm/IR/GlobalVariable.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/Intrinsics.h"
#include "llvm/IR/IntrinsicsAMDGPU.h"
#include "llvm/IR/LegacyPassManager.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/NoFolder.h"
#include "llvm/IR/Verifier.h"
#include "llvm/MC/TargetRegistry.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Support/CodeGen.h"
#include "llvm/Support/Alignment.h"
#include "llvm/Support/CrashRecoveryContext.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/FileSystem.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/TargetSelect.h"
#include "llvm/Support/raw_ostream.h"
#include "llvm/Target/TargetMachine.h"
#include "llvm/TargetParser/Triple.h"
#include "llvm/Transforms/Utils/Cloning.h"
#include "llvm/Transforms/Utils/Local.h"

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
#include <random>
#include <string>
#include <system_error>
#include <unistd.h>
#include <vector>

LLD_HAS_DRIVER(elf)

using namespace llvm;

namespace {

constexpr unsigned ThreadsPerBlock = 256;
constexpr unsigned InputCount = 256;
constexpr unsigned MaxIRCFGBlocks = 2048;

constexpr StringRef DataLayout =
    "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-"
    "p6:32:32-p7:160:256:256:32-p8:128:128-p9:192:256:256:32-"
    "i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-"
    "v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9";

uint64_t random64(std::minstd_rand &Gen) {
  uint64_t V = 0;
  for (unsigned I = 0; I < 8; ++I)
    V |= (static_cast<uint64_t>(Gen() & 0xffu) << (I * 8));
  return V;
}

uint64_t randomInteresting64(std::minstd_rand &Gen) {
  switch (Gen() % 12) {
  case 0:
    return 0;
  case 1:
    return 1;
  case 2:
    return ~0ull;
  case 3:
    return 0x7fffffffull;
  case 4:
    return 0x80000000ull;
  case 5:
    return 0xffffffffull;
  case 6:
    return 0x5555555555555555ull;
  case 7:
    return 0xaaaaaaaaaaaaaaaaull;
  case 8:
    return 1ull << (Gen() % 64);
  case 9:
    return (1ull << (Gen() % 63)) - 1;
  default:
    return random64(Gen);
  }
}

bool envFlag(const char *Name, bool Default) {
  const char *Value = std::getenv(Name);
  if (!Value || !*Value)
    return Default;
  return std::strcmp(Value, "0") != 0 && std::strcmp(Value, "false") != 0 &&
         std::strcmp(Value, "False") != 0 && std::strcmp(Value, "no") != 0 &&
         std::strcmp(Value, "off") != 0;
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

struct CompileResult {
  SmallVector<char, 0> Object;
  std::string FailureStage;
  int CrashRetCode = 0;
  bool Success = false;
  bool Crashed = false;
};

std::string moduleToString(Module &M) {
  std::string Text;
  raw_string_ostream OS(Text);
  M.print(OS, nullptr);
  return Text;
}

StringRef getCPU();

bool addModuleFlagIfMissing(Module &M, Module::ModFlagBehavior Behavior,
                            StringRef Key, uint32_t Value) {
  if (M.getModuleFlag(Key))
    return true;
  M.addModuleFlag(Behavior, Key, Value);
  return true;
}

bool addModuleFlagIfMissing(Module &M, Module::ModFlagBehavior Behavior,
                            StringRef Key, StringRef Value) {
  if (M.getModuleFlag(Key))
    return true;
  M.addModuleFlag(Behavior, Key, MDString::get(M.getContext(), Value));
  return true;
}

FunctionType *irKernelType(LLVMContext &Ctx) {
  Type *VoidTy = Type::getVoidTy(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *GlobalPtr = PointerType::get(Ctx, 1);
  return FunctionType::get(VoidTy, {GlobalPtr, GlobalPtr, I32}, false);
}

bool hasIRKernelSignature(const Function &F) {
  FunctionType *Expected = irKernelType(F.getContext());
  FunctionType *Actual = F.getFunctionType();
  if (Actual->getReturnType() != Expected->getReturnType() ||
      Actual->getNumParams() != Expected->getNumParams())
    return false;
  for (unsigned I = 0; I < Actual->getNumParams(); ++I)
    if (Actual->getParamType(I) != Expected->getParamType(I))
      return false;
  return true;
}

Function *findIRKernel(Module &M) {
  Function *F = M.getFunction("fuzz_kernel");
  if (!F)
    F = M.getFunction("fuzz_kernel_o0");
  if (!F)
    F = M.getFunction("fuzz_kernel_o2");
  if (!F || F->isDeclaration() || !hasIRKernelSignature(*F))
    return nullptr;
  return F;
}

ConstantInt *interestingI32(LLVMContext &Ctx, std::minstd_rand &Gen) {
  static constexpr std::array<uint32_t, 12> Values = {
      0, 1, 2, 3, 7, 31, 0xff, 0xffff, 0x7fffffff, 0x80000000u,
      0xffffffffu, 0x55555555u};
  if ((Gen() % 4) != 0)
    return ci32(Ctx, Values[Gen() % Values.size()]);
  return ci32(Ctx, static_cast<uint32_t>(random64(Gen)));
}

std::unique_ptr<Module> createIRSkeletonModule(LLVMContext &Ctx,
                                               StringRef CPU) {
  auto M = std::make_unique<Module>("fuzzx_amdgpu_ir_diff", Ctx);
  M->setTargetTriple(Triple("amdgcn-amd-amdhsa"));
  M->setDataLayout(DataLayout);
  M->addModuleFlag(Module::Error, "amdhsa_code_object_version", 600);
  M->addModuleFlag(Module::Error, "amdgpu_printf_kind",
                   MDString::get(Ctx, "hostcall"));
  M->addModuleFlag(Module::Max, "PIC Level", 2);

  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Function *F = Function::Create(irKernelType(Ctx),
                                 GlobalValue::ExternalLinkage, "fuzz_kernel",
                                 *M);
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
  IRBuilder<NoFolder> B(Entry);

  Function *Workgroup =
      Intrinsic::getOrInsertDeclaration(M.get(), Intrinsic::amdgcn_workgroup_id_x);
  Function *Workitem =
      Intrinsic::getOrInsertDeclaration(M.get(), Intrinsic::amdgcn_workitem_id_x);
  Value *WG = B.CreateCall(Workgroup, {}, "wg");
  Value *WI = B.CreateCall(Workitem, {}, "wi");
  Value *Idx = B.CreateAdd(B.CreateMul(WG, ci32(Ctx, ThreadsPerBlock)), WI,
                           "idx");
  Value *Ok = B.CreateICmpULT(Idx, N, "ok");
  B.CreateCondBr(Ok, Body, Exit);

  B.SetInsertPoint(Body);
  Value *Idx64 = B.CreateZExt(Idx, I64, "idx64");
  Value *InPtr = B.CreateGEP(I32, In, Idx64, "in.ptr");
  Value *V = B.CreateLoad(I32, InPtr, "v");
  Value *Mixed = B.CreateXor(V, B.CreateMul(Idx, ci32(Ctx, 0x9e3779b9u)),
                             "mix");
  Value *OutPtr = B.CreateGEP(I32, Out, Idx64, "out.ptr");
  B.CreateStore(Mixed, OutPtr);
  B.CreateBr(Exit);

  B.SetInsertPoint(Exit);
  B.CreateRetVoid();
  return M;
}

bool isAllowedIRIntrinsic(Intrinsic::ID ID) {
  switch (ID) {
  case Intrinsic::amdgcn_workgroup_id_x:
  case Intrinsic::amdgcn_workitem_id_x:
  case Intrinsic::ctlz:
  case Intrinsic::cttz:
  case Intrinsic::ctpop:
  case Intrinsic::bswap:
  case Intrinsic::bitreverse:
  case Intrinsic::abs:
  case Intrinsic::umin:
  case Intrinsic::umax:
  case Intrinsic::smin:
  case Intrinsic::smax:
  case Intrinsic::uadd_sat:
  case Intrinsic::usub_sat:
  case Intrinsic::sadd_sat:
  case Intrinsic::ssub_sat:
  case Intrinsic::uadd_with_overflow:
  case Intrinsic::usub_with_overflow:
  case Intrinsic::umul_with_overflow:
  case Intrinsic::sadd_with_overflow:
  case Intrinsic::ssub_with_overflow:
  case Intrinsic::smul_with_overflow:
  case Intrinsic::fshl:
  case Intrinsic::fshr:
  case Intrinsic::amdgcn_fma_legacy:
  case Intrinsic::amdgcn_frexp_exp:
  case Intrinsic::amdgcn_frexp_mant:
  case Intrinsic::amdgcn_fract:
  case Intrinsic::amdgcn_cvt_pkrtz:
  case Intrinsic::amdgcn_cvt_pknorm_i16:
  case Intrinsic::amdgcn_cvt_pknorm_u16:
  case Intrinsic::amdgcn_cvt_pk_i16:
  case Intrinsic::amdgcn_cvt_pk_u16:
  case Intrinsic::amdgcn_cvt_pk_u8_f32:
  case Intrinsic::amdgcn_class:
  case Intrinsic::amdgcn_fmed3:
  case Intrinsic::amdgcn_ubfe:
  case Intrinsic::amdgcn_sbfe:
  case Intrinsic::amdgcn_lerp:
  case Intrinsic::amdgcn_sad_u8:
  case Intrinsic::amdgcn_msad_u8:
  case Intrinsic::amdgcn_sad_hi_u8:
  case Intrinsic::amdgcn_sad_u16:
  case Intrinsic::amdgcn_qsad_pk_u16_u8:
  case Intrinsic::amdgcn_mqsad_pk_u16_u8:
  case Intrinsic::amdgcn_mqsad_u32_u8:
  case Intrinsic::amdgcn_mul_i24:
  case Intrinsic::amdgcn_mul_u24:
  case Intrinsic::amdgcn_mulhi_i24:
  case Intrinsic::amdgcn_mulhi_u24:
  case Intrinsic::amdgcn_alignbyte:
  case Intrinsic::amdgcn_sffbh:
  case Intrinsic::amdgcn_mbcnt_lo:
  case Intrinsic::amdgcn_mbcnt_hi:
  case Intrinsic::amdgcn_perm:
  case Intrinsic::amdgcn_bitop3:
  case Intrinsic::amdgcn_readfirstlane:
  case Intrinsic::amdgcn_wave_reduce_umin:
  case Intrinsic::amdgcn_wave_reduce_min:
  case Intrinsic::amdgcn_wave_reduce_umax:
  case Intrinsic::amdgcn_wave_reduce_max:
  case Intrinsic::amdgcn_wave_reduce_add:
  case Intrinsic::amdgcn_wave_reduce_and:
  case Intrinsic::amdgcn_wave_reduce_or:
  case Intrinsic::amdgcn_wave_reduce_xor:
  case Intrinsic::amdgcn_sdot2:
  case Intrinsic::amdgcn_udot2:
  case Intrinsic::amdgcn_sdot4:
  case Intrinsic::amdgcn_udot4:
  case Intrinsic::amdgcn_sudot4:
  case Intrinsic::amdgcn_sdot8:
  case Intrinsic::amdgcn_udot8:
  case Intrinsic::amdgcn_sudot8:
    return true;
  default:
    return false;
  }
}

bool isKnownNonZeroInteger(const Value *V) {
  if (const auto *C = dyn_cast<ConstantInt>(V))
    return C->getType()->isIntegerTy() && !C->isZero();
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy())
    return false;
  if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(0)))
    return !C->isZero();
  if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(1)))
    return !C->isZero();
  return false;
}

bool isKnownNonZeroFP(const Value *V) {
  if (const auto *C = dyn_cast<ConstantFP>(V))
    return !C->isZero();
  if (const auto *UIToFP = dyn_cast<UIToFPInst>(V))
    return isKnownNonZeroInteger(UIToFP->getOperand(0));
  if (const auto *SIToFP = dyn_cast<SIToFPInst>(V))
    return isKnownNonZeroInteger(SIToFP->getOperand(0));
  if (const auto *Ext = dyn_cast<FPExtInst>(V))
    return isKnownNonZeroFP(Ext->getOperand(0));
  return false;
}

bool isKnownNonNegativeI32(const Value *V) {
  if (const auto *C = dyn_cast<ConstantInt>(V))
    return C->getType()->isIntegerTy(32) && !C->isNegative();

  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || !BO->getType()->isIntegerTy(32))
    return false;

  if (BO->getOpcode() == Instruction::And) {
    for (const Value *Op : BO->operands()) {
      const auto *C = dyn_cast<ConstantInt>(Op);
      if (C && !C->getValue().isSignBitSet())
        return true;
    }
  }

  if (BO->getOpcode() == Instruction::Or)
    return isKnownNonNegativeI32(BO->getOperand(0)) &&
           isKnownNonNegativeI32(BO->getOperand(1));

  return false;
}

bool isKnownPositiveI32(const Value *V) {
  if (const auto *C = dyn_cast<ConstantInt>(V))
    return C->getType()->isIntegerTy(32) && C->getSExtValue() > 0;

  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;

  return (isKnownPositiveI32(BO->getOperand(0)) &&
          isKnownNonNegativeI32(BO->getOperand(1))) ||
         (isKnownPositiveI32(BO->getOperand(1)) &&
          isKnownNonNegativeI32(BO->getOperand(0)));
}

bool isKnownUnsignedI32AtMost(const Value *V, uint64_t Limit) {
  if (const auto *C = dyn_cast<ConstantInt>(V))
    return C->getType()->isIntegerTy(32) && C->getValue().ule(Limit);

  if (const auto *BO = dyn_cast<BinaryOperator>(V)) {
    if (!BO->getType()->isIntegerTy(32))
      return false;
    if (BO->getOpcode() == Instruction::And) {
      for (const Value *Op : BO->operands()) {
        const auto *C = dyn_cast<ConstantInt>(Op);
        if (C && C->getValue().ule(Limit))
          return true;
      }
    }
  }

  if (const auto *Sel = dyn_cast<SelectInst>(V))
    return Sel->getType()->isIntegerTy(32) &&
           isKnownUnsignedI32AtMost(Sel->getTrueValue(), Limit) &&
           isKnownUnsignedI32AtMost(Sel->getFalseValue(), Limit);

  return false;
}

bool isM040SmallOddPositiveDenominator(const Value *V) {
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;

  for (unsigned Idx = 0; Idx != 2; ++Idx) {
    const auto *C = dyn_cast<ConstantInt>(BO->getOperand(Idx));
    if (!C || C->isZero() || C->getValue().ugt(255))
      continue;
    if (isKnownUnsignedI32AtMost(BO->getOperand(1 - Idx), 255))
      return true;
  }
  return false;
}

unsigned integerScalarWidth(Type *Ty) {
  if (Ty->isIntegerTy())
    return Ty->getIntegerBitWidth();
  if (auto *VT = dyn_cast<VectorType>(Ty)) {
    Type *EltTy = VT->getElementType();
    if (EltTy->isIntegerTy())
      return EltTy->getIntegerBitWidth();
  }
  return 0;
}

bool isConstantIntOrVectorBelow(const Value *V, unsigned Limit) {
  if (const auto *C = dyn_cast<ConstantInt>(V))
    return C->getValue().ult(Limit);
  const auto *C = dyn_cast<Constant>(V);
  const auto *VT = V ? dyn_cast<FixedVectorType>(V->getType()) : nullptr;
  if (!C || !VT)
    return false;
  for (unsigned I = 0, E = VT->getNumElements(); I != E; ++I) {
    auto *Elt = dyn_cast_or_null<ConstantInt>(C->getAggregateElement(I));
    if (!Elt || !Elt->getValue().ult(Limit))
      return false;
  }
  return true;
}

bool isFixedIntVectorType(Type *Ty, unsigned BitWidth) {
  auto *VT = dyn_cast<FixedVectorType>(Ty);
  return VT && VT->getElementType()->isIntegerTy(BitWidth) &&
         (VT->getNumElements() == 2 || VT->getNumElements() == 4);
}

bool isAllowedIntVectorType(Type *Ty) {
  auto *VT = dyn_cast<FixedVectorType>(Ty);
  if (!VT)
    return false;
  Type *EltTy = VT->getElementType();
  unsigned Lanes = VT->getNumElements();
  if (EltTy->isIntegerTy(32))
    return Lanes == 2 || Lanes == 4;
  if (EltTy->isIntegerTy(16))
    return Lanes == 4 || Lanes == 8;
  if (EltTy->isIntegerTy(8))
    return Lanes == 4 || Lanes == 8;
  return false;
}

bool isAllowedFPScalarType(Type *Ty) {
  return Ty->isHalfTy() || Ty->isFloatTy() || Ty->isDoubleTy();
}

bool isAllowedFPVectorType(Type *Ty) {
  auto *VT = dyn_cast<FixedVectorType>(Ty);
  return VT &&
         (VT->getElementType()->isHalfTy() ||
          VT->getElementType()->isFloatTy()) &&
         (VT->getNumElements() == 2 || VT->getNumElements() == 4);
}

bool isAllowedVectorValueType(Type *Ty) {
  return isAllowedIntVectorType(Ty) || isAllowedFPVectorType(Ty);
}

bool isMatchingI1VectorType(Type *CondTy, Type *ValueTy) {
  auto *CondVT = dyn_cast<FixedVectorType>(CondTy);
  auto *ValueVT = dyn_cast<FixedVectorType>(ValueTy);
  return CondVT && ValueVT && CondVT->getElementType()->isIntegerTy(1) &&
         CondVT->getNumElements() == ValueVT->getNumElements();
}

bool isValidVectorLaneIndex(Type *VecTy, const Value *Index) {
  auto *VT = dyn_cast<FixedVectorType>(VecTy);
  auto *C = dyn_cast<ConstantInt>(Index);
  return VT && C && C->getZExtValue() < VT->getNumElements();
}

bool isValidVectorInstruction(const Instruction &I) {
  if (const auto *Shuffle = dyn_cast<ShuffleVectorInst>(&I)) {
    auto *ResultTy = dyn_cast<FixedVectorType>(Shuffle->getType());
    auto *OpTy = dyn_cast<FixedVectorType>(Shuffle->getOperand(0)->getType());
    if (!ResultTy || !OpTy || Shuffle->getOperand(1)->getType() != OpTy ||
        !isAllowedVectorValueType(ResultTy) || !isAllowedVectorValueType(OpTy))
      return false;
    unsigned MaxLane = 2 * OpTy->getNumElements();
    for (int Lane : Shuffle->getShuffleMask())
      if (Lane < 0 || static_cast<unsigned>(Lane) >= MaxLane)
        return false;
    return true;
  }
  if (const auto *Insert = dyn_cast<InsertElementInst>(&I)) {
    auto *VT = dyn_cast<FixedVectorType>(Insert->getType());
    return VT && isAllowedVectorValueType(Insert->getType()) &&
           Insert->getOperand(1)->getType() == VT->getElementType() &&
           isValidVectorLaneIndex(Insert->getType(), Insert->getOperand(2));
  }
  if (const auto *Extract = dyn_cast<ExtractElementInst>(&I)) {
    auto *VT = dyn_cast<FixedVectorType>(Extract->getVectorOperandType());
    return VT && Extract->getType() == VT->getElementType() &&
           isAllowedVectorValueType(Extract->getVectorOperandType()) &&
           isValidVectorLaneIndex(Extract->getVectorOperandType(),
                                  Extract->getIndexOperand());
  }
  if (const auto *BO = dyn_cast<BinaryOperator>(&I)) {
    if (!BO->getType()->isVectorTy())
      return true;
    if (isAllowedIntVectorType(BO->getType())) {
      if (BO->isShift())
        return isConstantIntOrVectorBelow(BO->getOperand(1),
                                          integerScalarWidth(BO->getType()));
      return true;
    }
    return isAllowedFPVectorType(BO->getType()) &&
           (BO->getOpcode() == Instruction::FAdd ||
            BO->getOpcode() == Instruction::FSub ||
            BO->getOpcode() == Instruction::FMul);
  }
  if (const auto *Cmp = dyn_cast<ICmpInst>(&I)) {
    if (!Cmp->getOperand(0)->getType()->isVectorTy())
      return true;
    return isAllowedIntVectorType(Cmp->getOperand(0)->getType()) &&
           isMatchingI1VectorType(Cmp->getType(),
                                  Cmp->getOperand(0)->getType());
  }
  if (const auto *Cmp = dyn_cast<FCmpInst>(&I)) {
    if (!Cmp->getOperand(0)->getType()->isVectorTy())
      return true;
    return isAllowedFPVectorType(Cmp->getOperand(0)->getType()) &&
           isMatchingI1VectorType(Cmp->getType(),
                                  Cmp->getOperand(0)->getType());
  }
  if (const auto *Sel = dyn_cast<SelectInst>(&I)) {
    if (!Sel->getType()->isVectorTy())
      return true;
    return isAllowedVectorValueType(Sel->getType()) &&
           isMatchingI1VectorType(Sel->getCondition()->getType(),
                                  Sel->getType());
  }
  return true;
}

bool isOverflowIntrinsic(Intrinsic::ID ID) {
  switch (ID) {
  case Intrinsic::uadd_with_overflow:
  case Intrinsic::usub_with_overflow:
  case Intrinsic::umul_with_overflow:
  case Intrinsic::sadd_with_overflow:
  case Intrinsic::ssub_with_overflow:
  case Intrinsic::smul_with_overflow:
    return true;
  default:
    return false;
  }
}

bool isValidAggregateInstruction(const Instruction &I) {
  const auto *Extract = dyn_cast<ExtractValueInst>(&I);
  if (!Extract)
    return true;
  if (Extract->getNumIndices() != 1)
    return false;
  const auto *Call = dyn_cast<CallInst>(Extract->getAggregateOperand());
  if (!Call)
    return false;
  const Function *Callee = Call->getCalledFunction();
  if (!Callee || !Callee->isIntrinsic() ||
      !isOverflowIntrinsic(Callee->getIntrinsicID()))
    return false;
  unsigned Index = *Extract->idx_begin();
  if (Index == 0)
    return Extract->getType()->isIntegerTy(32);
  if (Index == 1)
    return Extract->getType()->isIntegerTy(1);
  return false;
}

bool isValidFPConversionInstruction(const Instruction &I) {
  const auto *UIToFP = dyn_cast<UIToFPInst>(&I);
  if (!UIToFP)
    return true;
  if (!UIToFP->getOperand(0)->getType()->isIntegerTy(32))
    return false;
  Type *DestTy = UIToFP->getType();
  if (DestTy->isHalfTy())
    return isKnownUnsignedI32AtMost(UIToFP->getOperand(0), 127);
  if (DestTy->isFloatTy())
    return isKnownUnsignedI32AtMost(UIToFP->getOperand(0), 1023);
  return false;
}

bool isAllowedIRInstruction(const Instruction &I) {
  if (isa<InsertElementInst, ExtractElementInst, ShuffleVectorInst,
          ExtractValueInst>(&I))
    return true;
  if (isa<BranchInst, SwitchInst, ReturnInst, LoadInst, StoreInst,
          GetElementPtrInst, ZExtInst, SExtInst, TruncInst, UIToFPInst,
          SIToFPInst, FPToUIInst, FPToSIInst, FPExtInst, FPTruncInst,
          BitCastInst,
          ICmpInst, FCmpInst, PHINode, SelectInst>(&I))
    return true;
  if (auto *BO = dyn_cast<BinaryOperator>(&I)) {
    switch (BO->getOpcode()) {
    case Instruction::Add:
    case Instruction::Sub:
    case Instruction::Mul:
    case Instruction::Xor:
    case Instruction::And:
    case Instruction::Or:
      return true;
    case Instruction::FAdd:
    case Instruction::FSub:
    case Instruction::FMul:
      return isAllowedFPScalarType(BO->getType()) ||
             isAllowedFPVectorType(BO->getType());
    case Instruction::FDiv:
      return isAllowedFPScalarType(BO->getType()) &&
             isKnownNonZeroFP(BO->getOperand(1));
    case Instruction::UDiv:
    case Instruction::URem:
      if (BO->hasName() && BO->getName().starts_with("fuzz.load.idx"))
        return true;
      return isKnownNonZeroInteger(BO->getOperand(1));
    case Instruction::SDiv:
    case Instruction::SRem:
      return isKnownPositiveI32(BO->getOperand(1));
    case Instruction::Shl:
    case Instruction::LShr:
    case Instruction::AShr:
      if (unsigned Width = integerScalarWidth(BO->getType())) {
        if (isConstantIntOrVectorBelow(BO->getOperand(1), Width))
          return true;
        return BO->getType()->isIntegerTy(32) &&
               isKnownUnsignedI32AtMost(BO->getOperand(1), Width - 1);
      }
      return false;
    default:
      return false;
    }
  }
  if (const auto *Call = dyn_cast<CallInst>(&I)) {
    const Function *Callee = Call->getCalledFunction();
    if (!Callee || !Callee->isIntrinsic())
      return false;
    return isAllowedIRIntrinsic(Callee->getIntrinsicID());
  }
  return false;
}

bool hasExactName(const Value *V, StringRef Name) {
  return V && V->hasName() && V->getName() == Name;
}

bool isSmallLoopTripCount(const Value *V) {
  const auto *C = dyn_cast<ConstantInt>(V);
  if (C && C->getType()->isIntegerTy(32)) {
    uint64_t Trip = C->getZExtValue();
    return Trip >= 1 && Trip <= 8;
  }
  return hasExactName(V, "fuzz.loop.trip");
}

bool hasI32ConstantValue(const Value *V, uint32_t Expected) {
  const auto *C = dyn_cast<ConstantInt>(V);
  return C && C->getType()->isIntegerTy(32) && C->getZExtValue() == Expected;
}

bool isValidLoopControlInstruction(const Instruction &I) {
  if (!I.hasName() || !I.getName().starts_with("fuzz.loop."))
    return true;

  if (I.getName() == "fuzz.loop.cond") {
    const auto *Cmp = dyn_cast<ICmpInst>(&I);
    return Cmp && Cmp->getPredicate() == ICmpInst::ICMP_ULT &&
           hasExactName(Cmp->getOperand(0), "fuzz.loop.iv") &&
           isSmallLoopTripCount(Cmp->getOperand(1));
  }

  if (I.getName() == "fuzz.loop.next") {
    const auto *BO = dyn_cast<BinaryOperator>(&I);
    return BO && BO->getOpcode() == Instruction::Add &&
           hasExactName(BO->getOperand(0), "fuzz.loop.iv") &&
           hasI32ConstantValue(BO->getOperand(1), 1);
  }

  if (I.getName() == "fuzz.loop.iv") {
    const auto *Phi = dyn_cast<PHINode>(&I);
    if (!Phi || Phi->getNumIncomingValues() != 2)
      return false;
    bool HasStart = false;
    bool HasBackedge = false;
    for (unsigned Idx = 0; Idx != 2; ++Idx) {
      HasStart |= hasI32ConstantValue(Phi->getIncomingValue(Idx), 0);
      HasBackedge |=
          hasExactName(Phi->getIncomingValue(Idx), "fuzz.loop.next");
    }
    return HasStart && HasBackedge;
  }

  return true;
}

void scrubPoisonAnnotations(Module &M) {
  for (Function &F : M) {
    for (BasicBlock &BB : F) {
      for (Instruction &I : BB) {
        I.dropPoisonGeneratingAnnotations();
        if (auto *GEP = dyn_cast<GetElementPtrInst>(&I))
          GEP->setIsInBounds(false);
        if (auto *Call = dyn_cast<CallInst>(&I)) {
          Function *Callee = Call->getCalledFunction();
          if (!Callee || !Callee->isIntrinsic())
            continue;
          Intrinsic::ID ID = Callee->getIntrinsicID();
          if (ID == Intrinsic::ctlz || ID == Intrinsic::cttz) {
            if (Call->arg_size() == 2)
              Call->setArgOperand(1, ConstantInt::getFalse(M.getContext()));
          } else if (ID == Intrinsic::abs) {
            if (Call->arg_size() == 2)
              Call->setArgOperand(1, ConstantInt::getFalse(M.getContext()));
          }
        }
      }
    }
  }
}

bool triggersM001AShrI16ZExt(const Instruction &I) {
  const auto *ZExt = dyn_cast<ZExtInst>(&I);
  if (!ZExt || !ZExt->getType()->isIntegerTy(32) ||
      !ZExt->getOperand(0)->getType()->isIntegerTy(16))
    return false;
  const auto *BO = dyn_cast<BinaryOperator>(ZExt->getOperand(0));
  if (!BO || BO->getOpcode() != Instruction::AShr ||
      !BO->getType()->isIntegerTy(16))
    return false;
  return isConstantIntOrVectorBelow(BO->getOperand(1), 16);
}

bool triggersM015M016ScalarFshl(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call || !Call->getType()->isIntegerTy(32))
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::fshl;
}

bool triggersM049VectorFshl(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call || !isa<FixedVectorType>(Call->getType()))
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::fshl;
}

bool triggersM051VectorFshr(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call || !isa<FixedVectorType>(Call->getType()))
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::fshr;
}

bool isAndWithOperand(const Value *MaybeAnd, const Value *Operand) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeAnd);
  if (!BO || BO->getOpcode() != Instruction::And)
    return false;
  return BO->getOperand(0) == Operand || BO->getOperand(1) == Operand;
}

bool isSubOfValueAndZero(const Value *MaybeSub, const Value *Base) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeSub);
  return BO && BO->getOpcode() == Instruction::Sub &&
         BO->getType()->isIntegerTy(32) && BO->getOperand(0) == Base &&
         hasI32ConstantValue(BO->getOperand(1), 0);
}

bool triggersM050AndSubZero(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isSubOfValueAndZero(BO->getOperand(0), BO->getOperand(1)) ||
         isSubOfValueAndZero(BO->getOperand(1), BO->getOperand(0));
}

bool isI32XorWithOperand(const Value *MaybeXor, const Value *Operand) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeXor);
  if (!BO || BO->getOpcode() != Instruction::Xor ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return BO->getOperand(0) == Operand || BO->getOperand(1) == Operand;
}

bool isM027MaskedBase(const Value *MaybeMasked, const Value *Base) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeMasked);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return (BO->getOperand(0) == Base &&
          isI32XorWithOperand(BO->getOperand(1), Base)) ||
         (BO->getOperand(1) == Base &&
          isI32XorWithOperand(BO->getOperand(0), Base));
}

bool isM027AndOfXorWithMaskedBase(const Value *MaybeAnd,
                                  const Value *Base) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeAnd);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return (isM027MaskedBase(BO->getOperand(0), Base) &&
          isI32XorWithOperand(BO->getOperand(1), BO->getOperand(0))) ||
         (isM027MaskedBase(BO->getOperand(1), Base) &&
          isI32XorWithOperand(BO->getOperand(0), BO->getOperand(1)));
}

bool triggersM027XorAndOr(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isM027AndOfXorWithMaskedBase(BO->getOperand(0), BO->getOperand(1)) ||
         isM027AndOfXorWithMaskedBase(BO->getOperand(1), BO->getOperand(0));
}

bool triggersM040SignedDivRem24(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || !BO->getType()->isIntegerTy(32))
    return false;
  if (BO->getOpcode() != Instruction::SDiv &&
      BO->getOpcode() != Instruction::SRem)
    return false;
  return isM040SmallOddPositiveDenominator(BO->getOperand(1)) &&
         !isKnownUnsignedI32AtMost(BO->getOperand(0), 0x7fffff);
}

bool isUnsignedMaxWithOperand(const Value *MaybeMax, const Value *Operand) {
  if (const auto *Call = dyn_cast<CallInst>(MaybeMax)) {
    const Function *Callee = Call->getCalledFunction();
    return Callee && Callee->isIntrinsic() &&
           Callee->getIntrinsicID() == Intrinsic::umax &&
           Call->arg_size() == 2 &&
           (Call->getArgOperand(0) == Operand ||
            Call->getArgOperand(1) == Operand);
  }

  const auto *Sel = dyn_cast<SelectInst>(MaybeMax);
  if (!Sel || !Sel->getType()->isIntegerTy(32))
    return false;

  const auto *Cmp = dyn_cast<ICmpInst>(Sel->getCondition());
  if (!Cmp)
    return false;

  const Value *LHS = Cmp->getOperand(0);
  const Value *RHS = Cmp->getOperand(1);
  const Value *TrueValue = Sel->getTrueValue();
  const Value *FalseValue = Sel->getFalseValue();
  switch (Cmp->getPredicate()) {
  case ICmpInst::ICMP_ULT:
  case ICmpInst::ICMP_ULE:
    return TrueValue == RHS && FalseValue == LHS &&
           (LHS == Operand || RHS == Operand);
  case ICmpInst::ICMP_UGT:
  case ICmpInst::ICMP_UGE:
    return TrueValue == LHS && FalseValue == RHS &&
           (LHS == Operand || RHS == Operand);
  default:
    return false;
  }
}

bool isXorWithOperand(const Value *MaybeXor, const Value *Operand,
                      const Value **Other = nullptr) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeXor);
  if (!BO || BO->getOpcode() != Instruction::Xor ||
      !BO->getType()->isIntegerTy(32))
    return false;
  if (BO->getOperand(0) == Operand) {
    if (Other)
      *Other = BO->getOperand(1);
    return true;
  }
  if (BO->getOperand(1) == Operand) {
    if (Other)
      *Other = BO->getOperand(0);
    return true;
  }
  return false;
}

bool isM026UMaxXorAndOperandPair(const Value *MaybeAndOperand,
                                 const Value *MaybeXorOperand) {
  const Value *Other = nullptr;
  return isXorWithOperand(MaybeXorOperand, MaybeAndOperand, &Other) &&
         isUnsignedMaxWithOperand(MaybeAndOperand, Other);
}

bool triggersM026UMaxXorAnd(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isM026UMaxXorAndOperandPair(BO->getOperand(0), BO->getOperand(1)) ||
         isM026UMaxXorAndOperandPair(BO->getOperand(1), BO->getOperand(0));
}

bool isI32NotOf(const Value *MaybeNot, const Value **Base = nullptr) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeNot);
  if (!BO || BO->getOpcode() != Instruction::Xor ||
      !BO->getType()->isIntegerTy(32))
    return false;
  if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(0))) {
    if (C->isMinusOne()) {
      if (Base)
        *Base = BO->getOperand(1);
      return true;
    }
  }
  if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(1))) {
    if (C->isMinusOne()) {
      if (Base)
        *Base = BO->getOperand(0);
      return true;
    }
  }
  return false;
}

const Value *stripI32AndAllOnes(const Value *V) {
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return V;
  if (hasI32ConstantValue(BO->getOperand(0), 0xffffffffu))
    return BO->getOperand(1);
  if (hasI32ConstantValue(BO->getOperand(1), 0xffffffffu))
    return BO->getOperand(0);
  return V;
}

bool isM052BlendRightArm(const Value *MaybeRight, const Value *Mask,
                         const BinaryOperator *MaskXor) {
  const auto *And = dyn_cast<BinaryOperator>(MaybeRight);
  if (!And || And->getOpcode() != Instruction::And ||
      !And->getType()->isIntegerTy(32))
    return false;

  auto IsXorOperand = [&](const Value *V) {
    return V == MaskXor->getOperand(0) || V == MaskXor->getOperand(1);
  };
  auto Matches = [&](const Value *MaybeNot, const Value *Other) {
    const Value *NotBase = nullptr;
    return isI32NotOf(MaybeNot, &NotBase) && NotBase == Mask &&
           IsXorOperand(Other);
  };
  return Matches(And->getOperand(0), And->getOperand(1)) ||
         Matches(And->getOperand(1), And->getOperand(0));
}

bool isM052TernaryBlendArmPair(const Value *MaybeMask,
                               const Value *MaybeRight) {
  const Value *Mask = stripI32AndAllOnes(MaybeMask);
  const auto *MaskXor = dyn_cast<BinaryOperator>(Mask);
  if (!MaskXor || MaskXor->getOpcode() != Instruction::Xor ||
      !MaskXor->getType()->isIntegerTy(32))
    return false;
  return isM052BlendRightArm(MaybeRight, Mask, MaskXor);
}

bool isM052TernaryBlend(const Value *MaybeOr) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeOr);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isM052TernaryBlendArmPair(BO->getOperand(0), BO->getOperand(1)) ||
         isM052TernaryBlendArmPair(BO->getOperand(1), BO->getOperand(0));
}

bool triggersM052TernaryBlendShift(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  const Value *MaybeBlend = nullptr;
  if (hasI32ConstantValue(BO->getOperand(0), 31))
    MaybeBlend = BO->getOperand(1);
  else if (hasI32ConstantValue(BO->getOperand(1), 31))
    MaybeBlend = BO->getOperand(0);
  return MaybeBlend && isM052TernaryBlend(MaybeBlend);
}

bool isM028UMaxOfMaskedNot(const Value *MaybeUMax, const Value *Not) {
  const auto *Call = dyn_cast<CallInst>(MaybeUMax);
  if (!Call)
    return false;
  const Function *Callee = Call->getCalledFunction();
  if (!Callee || !Callee->isIntrinsic() ||
      Callee->getIntrinsicID() != Intrinsic::umax || Call->arg_size() != 2 ||
      !Call->getType()->isIntegerTy(32))
    return false;
  return isAndWithOperand(Call->getArgOperand(0), Not) ||
         isAndWithOperand(Call->getArgOperand(1), Not);
}

bool isM028AndOfBaseAndUMax(const Value *MaybeAnd, const Value *Base,
                            const Value *Not) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeAnd);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return (BO->getOperand(0) == Base &&
          isM028UMaxOfMaskedNot(BO->getOperand(1), Not)) ||
         (BO->getOperand(1) == Base &&
          isM028UMaxOfMaskedNot(BO->getOperand(0), Not));
}

bool isM028AndPair(const Value *MaybeAnd, const Value *MaybeNot) {
  const Value *Base = nullptr;
  return isI32NotOf(MaybeNot, &Base) &&
         isM028AndOfBaseAndUMax(MaybeAnd, Base, MaybeNot);
}

bool triggersM028UMaxAndNot(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isM028AndPair(BO->getOperand(0), BO->getOperand(1)) ||
         isM028AndPair(BO->getOperand(1), BO->getOperand(0));
}

bool isI32Fshl(const Value *V) {
  const auto *Call = dyn_cast<CallInst>(V);
  if (!Call || !Call->getType()->isIntegerTy(32))
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::fshl;
}

bool isAndWithFshl(const Value *V) {
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isI32Fshl(BO->getOperand(0)) || isI32Fshl(BO->getOperand(1));
}

bool isM029ComplementedFshlMask(const Value *V) {
  const Value *Base = nullptr;
  return isI32NotOf(V, &Base) && isAndWithFshl(Base);
}

bool isExtOf(const Value *MaybeExt, const Value *Base) {
  const auto *Ext = dyn_cast<CastInst>(MaybeExt);
  return Ext && (isa<SExtInst>(Ext) || isa<ZExtInst>(Ext)) &&
         Ext->getOperand(0) == Base;
}

bool isM029XorWithComplement(const Value *Y, const Value *X) {
  if (isXorWithOperand(Y, X))
    return true;
  const auto *Trunc = dyn_cast<TruncInst>(Y);
  if (!Trunc || !Trunc->getType()->isIntegerTy(32))
    return false;
  const auto *BO = dyn_cast<BinaryOperator>(Trunc->getOperand(0));
  if (!BO || BO->getOpcode() != Instruction::Xor)
    return false;
  return isExtOf(BO->getOperand(0), X) || isExtOf(BO->getOperand(1), X);
}

bool isM029AndOfYAndX(const Value *MaybeAnd, const Value **Y = nullptr,
                      const Value **X = nullptr) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeAnd);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  for (unsigned I = 0; I != 2; ++I) {
    const Value *MaybeX = BO->getOperand(I);
    const Value *MaybeY = BO->getOperand(1 - I);
    if (isM029ComplementedFshlMask(MaybeX) &&
        isM029XorWithComplement(MaybeY, MaybeX)) {
      if (Y)
        *Y = MaybeY;
      if (X)
        *X = MaybeX;
      return true;
    }
  }
  return false;
}

bool isM029CompareForAndPath(const Value *Cond, const Value *Y, const Value *X,
                             bool AndOnTrue) {
  const auto *Cmp = dyn_cast<ICmpInst>(Cond);
  if (!Cmp || !Cmp->isSigned())
    return false;
  ICmpInst::Predicate Pred = Cmp->getPredicate();
  const Value *LHS = Cmp->getOperand(0);
  const Value *RHS = Cmp->getOperand(1);
  if (LHS == Y && RHS == X) {
    if (Pred == ICmpInst::ICMP_SLE || Pred == ICmpInst::ICMP_SLT)
      return AndOnTrue;
    if (Pred == ICmpInst::ICMP_SGT || Pred == ICmpInst::ICMP_SGE)
      return !AndOnTrue;
  }
  if (LHS == X && RHS == Y) {
    if (Pred == ICmpInst::ICMP_SGE || Pred == ICmpInst::ICMP_SGT)
      return AndOnTrue;
    if (Pred == ICmpInst::ICMP_SLT || Pred == ICmpInst::ICMP_SLE)
      return !AndOnTrue;
  }
  return false;
}

bool isM029Select(const SelectInst &Sel) {
  const Value *Y = nullptr;
  const Value *X = nullptr;
  if (isM029AndOfYAndX(Sel.getTrueValue(), &Y, &X) &&
      isM029CompareForAndPath(Sel.getCondition(), Y, X, true))
    return true;
  if (isM029AndOfYAndX(Sel.getFalseValue(), &Y, &X) &&
      isM029CompareForAndPath(Sel.getCondition(), Y, X, false))
    return true;
  return false;
}

bool isM029Phi(const PHINode &Phi) {
  if (!Phi.getType()->isIntegerTy(32) || Phi.getNumIncomingValues() != 2)
    return false;
  for (unsigned I = 0; I != 2; ++I) {
    const Value *Y = nullptr;
    const Value *X = nullptr;
    if (!isM029AndOfYAndX(Phi.getIncomingValue(I), &Y, &X))
      continue;
    BasicBlock *AndBB = Phi.getIncomingBlock(I);
    BasicBlock *OtherBB = Phi.getIncomingBlock(1 - I);
    BasicBlock *Pred = AndBB->getSinglePredecessor();
    if (!Pred || OtherBB->getSinglePredecessor() != Pred)
      continue;
    const auto *Br = dyn_cast<BranchInst>(Pred->getTerminator());
    if (!Br || !Br->isConditional())
      continue;
    bool AndOnTrue = Br->getSuccessor(0) == AndBB;
    if ((AndOnTrue || Br->getSuccessor(1) == AndBB) &&
        isM029CompareForAndPath(Br->getCondition(), Y, X, AndOnTrue))
      return true;
  }
  return false;
}

bool triggersM029FshlSelectPhi(const Instruction &I) {
  if (const auto *Sel = dyn_cast<SelectInst>(&I))
    return isM029Select(*Sel);
  if (const auto *Phi = dyn_cast<PHINode>(&I))
    return isM029Phi(*Phi);
  return false;
}

bool isI32IntrinsicCall(const Value *V, Intrinsic::ID ID) {
  const auto *Call = dyn_cast<CallInst>(V);
  if (!Call || !Call->getType()->isIntegerTy(32))
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() && Callee->getIntrinsicID() == ID;
}

bool isM030CtlzZeroCompare(const Value *V) {
  const auto *Cmp = dyn_cast<ICmpInst>(V);
  if (!Cmp || !Cmp->isEquality())
    return false;
  return (isI32IntrinsicCall(Cmp->getOperand(0), Intrinsic::ctlz) &&
          isa<ConstantInt>(Cmp->getOperand(1)) &&
          cast<ConstantInt>(Cmp->getOperand(1))->isZero()) ||
         (isI32IntrinsicCall(Cmp->getOperand(1), Intrinsic::ctlz) &&
          isa<ConstantInt>(Cmp->getOperand(0)) &&
          cast<ConstantInt>(Cmp->getOperand(0))->isZero());
}

bool isM030SmallBitValue(const Value *V) {
  if (isI32IntrinsicCall(V, Intrinsic::ctpop))
    return true;
  const auto *ZExt = dyn_cast<ZExtInst>(V);
  return ZExt && ZExt->getType()->isIntegerTy(32) &&
         isM030CtlzZeroCompare(ZExt->getOperand(0));
}

bool isM030ShiftedI32(const Value *V) {
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::Shl ||
      !BO->getType()->isIntegerTy(32))
    return false;
  const auto *Shift = dyn_cast<ConstantInt>(BO->getOperand(1));
  return Shift && Shift->getValue().ult(32);
}

bool isM030AddOfShiftAndBit(const Value *MaybeAdd, const Value *Bit) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeAdd);
  if (!BO || BO->getOpcode() != Instruction::Add ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return (BO->getOperand(0) == Bit && isM030ShiftedI32(BO->getOperand(1))) ||
         (BO->getOperand(1) == Bit && isM030ShiftedI32(BO->getOperand(0)));
}

bool isM030SMinOfAddAndBit(const Value *MaybeSMin, const Value *Bit) {
  const auto *Call = dyn_cast<CallInst>(MaybeSMin);
  if (!Call || !Call->getType()->isIntegerTy(32))
    return false;
  const Function *Callee = Call->getCalledFunction();
  if (!Callee || !Callee->isIntrinsic() ||
      Callee->getIntrinsicID() != Intrinsic::smin || Call->arg_size() != 2)
    return false;
  return (Call->getArgOperand(0) == Bit &&
          isM030AddOfShiftAndBit(Call->getArgOperand(1), Bit)) ||
         (Call->getArgOperand(1) == Bit &&
          isM030AddOfShiftAndBit(Call->getArgOperand(0), Bit));
}

bool isM030OrValueForBit(const Value *MaybeOrValue, const Value *Bit) {
  return isM030AddOfShiftAndBit(MaybeOrValue, Bit) ||
         isM030SMinOfAddAndBit(MaybeOrValue, Bit);
}

bool triggersM030CtlzShlOrBitop3(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return (isM030SmallBitValue(BO->getOperand(0)) &&
          isM030OrValueForBit(BO->getOperand(1), BO->getOperand(0))) ||
         (isM030SmallBitValue(BO->getOperand(1)) &&
          isM030OrValueForBit(BO->getOperand(0), BO->getOperand(1)));
}

bool isI32ExtractOfVectorOr(const Value *V, const BinaryOperator **Or,
                            uint64_t *Lane) {
  const auto *Extract = dyn_cast<ExtractElementInst>(V);
  if (!Extract || !Extract->getType()->isIntegerTy(32))
    return false;
  const auto *Index = dyn_cast<ConstantInt>(Extract->getIndexOperand());
  const auto *BO = dyn_cast<BinaryOperator>(Extract->getVectorOperand());
  if (!Index || !BO || BO->getOpcode() != Instruction::Or ||
      !isFixedIntVectorType(BO->getType(), 32))
    return false;
  if (Or)
    *Or = BO;
  if (Lane)
    *Lane = Index->getZExtValue();
  return true;
}

bool triggersM031VectorOrExtractSub(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Sub ||
      !BO->getType()->isIntegerTy(32))
    return false;

  const BinaryOperator *LOr = nullptr;
  const BinaryOperator *ROr = nullptr;
  uint64_t LLane = 0;
  uint64_t RLane = 0;
  return isI32ExtractOfVectorOr(BO->getOperand(0), &LOr, &LLane) &&
         isI32ExtractOfVectorOr(BO->getOperand(1), &ROr, &RLane) &&
         LOr == ROr && LLane != RLane;
}

bool isVectorSelect(const Value *V) {
  const auto *Sel = dyn_cast<SelectInst>(V);
  return Sel && Sel->getType()->isVectorTy();
}

bool dependsOnVectorSelect(const Value *V, SmallPtrSetImpl<const Value *> &Seen,
                           unsigned Depth = 0) {
  if (!V || Depth > 64 || !Seen.insert(V).second)
    return false;
  if (isVectorSelect(V))
    return true;
  const auto *I = dyn_cast<Instruction>(V);
  if (!I)
    return false;
  for (const Use &Op : I->operands())
    if (dependsOnVectorSelect(Op.get(), Seen, Depth + 1))
      return true;
  return false;
}

bool triggersM032LoopVectorSelect(const Instruction &I) {
  const auto *Phi = dyn_cast<PHINode>(&I);
  if (!Phi || !Phi->getType()->isIntegerTy(32) ||
      !Phi->hasName() || !Phi->getName().starts_with("fuzz.loop.acc"))
    return false;

  for (unsigned Idx = 0, End = Phi->getNumIncomingValues(); Idx != End; ++Idx) {
    SmallPtrSet<const Value *, 32> Seen;
    if (dependsOnVectorSelect(Phi->getIncomingValue(Idx), Seen))
      return true;
  }
  return false;
}

bool triggersM035WaveReduceXor(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call)
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::amdgcn_wave_reduce_xor;
}

bool triggersM036WaveReduceAdd(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call)
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::amdgcn_wave_reduce_add;
}

bool triggersM039SExtI8HighBytePack(const Instruction &I) {
  const auto *SExt = dyn_cast<SExtInst>(&I);
  if (!SExt || !SExt->getType()->isIntegerTy(32) ||
      !SExt->getOperand(0)->getType()->isIntegerTy(8))
    return false;

  for (const User *U : SExt->users()) {
    const auto *BO = dyn_cast<BinaryOperator>(U);
    if (!BO || BO->getOpcode() != Instruction::LShr ||
        !BO->getType()->isIntegerTy(32))
      continue;
    const auto *Shift = dyn_cast<ConstantInt>(BO->getOperand(1));
    if (Shift && (Shift->getZExtValue() == 16 ||
                  Shift->getZExtValue() == 24))
      return true;
  }
  return false;
}

bool isI32AShrByConstant(const Value *V) {
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::AShr ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isConstantIntOrVectorBelow(BO->getOperand(1), 32);
}

bool isI32LShrOfAShrBy(const Value *V, uint64_t Shift) {
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::LShr ||
      !BO->getType()->isIntegerTy(32) ||
      !hasI32ConstantValue(BO->getOperand(1), Shift))
    return false;
  return isI32AShrByConstant(BO->getOperand(0));
}

const Value *getI32AndOperandWithConstant(const BinaryOperator &BO,
                                          uint32_t Mask) {
  if (BO.getOpcode() != Instruction::And || !BO.getType()->isIntegerTy(32))
    return nullptr;
  if (hasI32ConstantValue(BO.getOperand(0), Mask))
    return BO.getOperand(1);
  if (hasI32ConstantValue(BO.getOperand(1), Mask))
    return BO.getOperand(0);
  return nullptr;
}

bool triggersM041AShrHighBytePack(const Instruction &I) {
  if (const auto *ZExt = dyn_cast<ZExtInst>(&I)) {
    if (!ZExt->getType()->isIntegerTy(32) ||
        !ZExt->getOperand(0)->getType()->isIntegerTy(8))
      return false;
    const auto *Trunc = dyn_cast<TruncInst>(ZExt->getOperand(0));
    return Trunc && isI32LShrOfAShrBy(Trunc->getOperand(0), 24);
  }

  const auto *And = dyn_cast<BinaryOperator>(&I);
  if (!And)
    return false;
  if (const Value *Masked = getI32AndOperandWithConstant(*And, 0x00ff0000))
    return isI32LShrOfAShrBy(Masked, 8);
  if (const Value *Masked = getI32AndOperandWithConstant(*And, 0x000000ff))
    return isI32LShrOfAShrBy(Masked, 24);
  return false;
}

bool triggersM043SelfXor(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  return BO && BO->getOpcode() == Instruction::Xor &&
         BO->getType()->isIntegerTy(32) &&
         BO->getOperand(0) == BO->getOperand(1);
}

bool isIdentityShuffleOf(const Value *MaybeShuffle, const Value *Operand) {
  const auto *Shuffle = dyn_cast<ShuffleVectorInst>(MaybeShuffle);
  if (!Shuffle || Shuffle->getOperand(0) != Operand)
    return false;
  const auto *VT = dyn_cast<FixedVectorType>(Shuffle->getType());
  if (!VT)
    return false;
  for (unsigned I = 0, E = VT->getNumElements(); I != E; ++I) {
    int Mask = Shuffle->getMaskValue(I);
    if (Mask != static_cast<int>(I))
      return false;
  }
  return true;
}

bool triggersM044V4I32SelfAnd(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !isFixedIntVectorType(BO->getType(), 32) ||
      cast<FixedVectorType>(BO->getType())->getNumElements() != 4)
    return false;
  return BO->getOperand(0) == BO->getOperand(1) ||
         isIdentityShuffleOf(BO->getOperand(0), BO->getOperand(1)) ||
         isIdentityShuffleOf(BO->getOperand(1), BO->getOperand(0));
}

bool isI32OrOneOf(const Value *MaybeOr, const Value *Operand) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeOr);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return (BO->getOperand(0) == Operand &&
          hasI32ConstantValue(BO->getOperand(1), 1)) ||
         (BO->getOperand(1) == Operand &&
          hasI32ConstantValue(BO->getOperand(0), 1));
}

bool triggersM045URemOrOne(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  return BO && BO->getOpcode() == Instruction::URem &&
         BO->getType()->isIntegerTy(32) &&
         isI32OrOneOf(BO->getOperand(1), BO->getOperand(0));
}

bool triggersM046V4I16Cttz(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call || !isFixedIntVectorType(Call->getType(), 16) ||
      cast<FixedVectorType>(Call->getType())->getNumElements() != 4)
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::cttz;
}

bool triggersM047V8I8Shl(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Shl)
    return false;
  const auto *VT = dyn_cast<FixedVectorType>(BO->getType());
  return VT && VT->getElementType()->isIntegerTy(8) &&
         VT->getNumElements() == 8;
}

bool triggersM048V8I8UAddSat(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call)
    return false;
  const auto *VT = dyn_cast<FixedVectorType>(Call->getType());
  if (!VT || !VT->getElementType()->isIntegerTy(8) ||
      VT->getNumElements() != 8)
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::uadd_sat;
}

bool isI32LShrByZeroOf(const Value *MaybeShift, const Value *Operand) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeShift);
  return BO && BO->getOpcode() == Instruction::LShr &&
         BO->getType()->isIntegerTy(32) && BO->getOperand(0) == Operand &&
         hasI32ConstantValue(BO->getOperand(1), 0);
}

bool triggersM042OrLShrZero(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isI32LShrByZeroOf(BO->getOperand(0), BO->getOperand(1)) ||
         isI32LShrByZeroOf(BO->getOperand(1), BO->getOperand(0));
}

bool triggersC001SUDotISELICE(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call)
    return false;
  const Function *Callee = Call->getCalledFunction();
  if (!Callee || !Callee->isIntrinsic())
    return false;
  Intrinsic::ID ID = Callee->getIntrinsicID();
  return ID == Intrinsic::amdgcn_sudot4 || ID == Intrinsic::amdgcn_sudot8;
}

bool triggersC002FMALegacyISELICE(const Instruction &I) {
  const auto *Call = dyn_cast<CallInst>(&I);
  if (!Call)
    return false;
  const Function *Callee = Call->getCalledFunction();
  return Callee && Callee->isIntrinsic() &&
         Callee->getIntrinsicID() == Intrinsic::amdgcn_fma_legacy;
}

bool hasName(const Value *V, StringRef Name) {
  return V && V->hasName() && V->getName() == Name;
}

bool hasNameStartingWith(const Value *V, StringRef Prefix) {
  return V && V->hasName() && V->getName().starts_with(Prefix);
}

bool isFuzzInputLoadIndex(const Value *V, Function &Kernel) {
  const auto *ZExt = dyn_cast<ZExtInst>(V);
  if (!ZExt || !hasNameStartingWith(ZExt, "fuzz.load.idx64") ||
      !ZExt->getType()->isIntegerTy(64))
    return false;

  const auto *URem = dyn_cast<BinaryOperator>(ZExt->getOperand(0));
  return URem && URem->getOpcode() == Instruction::URem &&
         hasNameStartingWith(URem, "fuzz.load.idx") &&
         URem->getOperand(1) == Kernel.getArg(2);
}

bool isFuzzInputLoadPtr(const Value *V) {
  return hasNameStartingWith(V, "fuzz.load.ptr");
}

bool validateMemoryShape(Function &Kernel) {
  bool SawBaseInGEP = false;
  bool SawBaseOutGEP = false;
  unsigned BaseLoads = 0;
  unsigned ExtraLoadGEPs = 0;
  unsigned ExtraLoads = 0;
  unsigned Stores = 0;
  for (BasicBlock &BB : Kernel) {
    for (Instruction &I : BB) {
      if (auto *GEP = dyn_cast<GetElementPtrInst>(&I)) {
        if (GEP->getSourceElementType() != Type::getInt32Ty(Kernel.getContext()))
          return false;
        if (hasName(GEP, "in.ptr") || hasName(GEP, "out.ptr")) {
          if (GEP->getNumIndices() != 1 ||
              !hasName(GEP->idx_begin()->get(), "idx64"))
            return false;
          unsigned ArgNo = hasName(GEP, "in.ptr") ? 0 : 1;
          if (GEP->getPointerOperand() != Kernel.getArg(ArgNo))
            return false;
          if (ArgNo == 0)
            SawBaseInGEP = true;
          else
            SawBaseOutGEP = true;
        } else if (isFuzzInputLoadPtr(GEP)) {
          if (GEP->getPointerOperand() != Kernel.getArg(0) ||
              GEP->getNumIndices() != 1 ||
              !isFuzzInputLoadIndex(GEP->idx_begin()->get(), Kernel))
            return false;
          ++ExtraLoadGEPs;
        } else {
          return false;
        }
      } else if (auto *Load = dyn_cast<LoadInst>(&I)) {
        if (!Load->getType()->isIntegerTy(32))
          return false;
        if (hasName(Load->getPointerOperand(), "in.ptr")) {
          ++BaseLoads;
        } else if (isFuzzInputLoadPtr(Load->getPointerOperand())) {
          ++ExtraLoads;
        } else {
          return false;
        }
      } else if (auto *Store = dyn_cast<StoreInst>(&I)) {
        ++Stores;
        if (!Store->getValueOperand()->getType()->isIntegerTy(32) ||
            !hasName(Store->getPointerOperand(), "out.ptr"))
          return false;
      }
    }
  }
  return SawBaseInGEP && SawBaseOutGEP && BaseLoads == 1 && Stores == 1 &&
         ExtraLoadGEPs == ExtraLoads && ExtraLoads <= 32;
}

bool validateIRCorpusModule(Module &M) {
  bool AllowM001 = envFlag("FUZZX_ALLOW_M001_ASHR_I16_ZEXT", false);
  bool AllowM015M016 =
      envFlag("FUZZX_ALLOW_M016_SCALAR_FSHL", false) ||
      envFlag("FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO", false);
  bool AllowM026 = envFlag("FUZZX_ALLOW_M026_UMAX_XOR_AND_HIGHBIT", false);
  bool AllowM027 = envFlag("FUZZX_ALLOW_M027_XOR_AND_OR", false);
  bool AllowM028 = envFlag("FUZZX_ALLOW_M028_UMAX_AND_NOT", false);
  bool AllowM029 = envFlag("FUZZX_ALLOW_M029_FSHL_SELECT_PHI", false);
  bool AllowM030 = envFlag("FUZZX_ALLOW_M030_CTLZ_SHL_OR_BITOP3", false);
  bool AllowM031 = envFlag("FUZZX_ALLOW_M031_VECTOR_OR_EXTRACT_SUB", false);
  bool AllowM032 = envFlag("FUZZX_ALLOW_M032_LOOP_VECTOR_SELECT", false);
  bool AllowM035 = envFlag("FUZZX_ALLOW_M035_WAVE_REDUCE_XOR", false);
  bool AllowM036 = envFlag("FUZZX_ALLOW_M036_WAVE_REDUCE_ADD", false);
  bool AllowM039 = envFlag("FUZZX_ALLOW_M039_SEXT_I8_HIGHBYTE", false);
  bool AllowM040 = envFlag("FUZZX_ALLOW_M040_SIGNED_DIVREM24", false);
  bool AllowM041 = envFlag("FUZZX_ALLOW_M041_ASHR_HIGHBYTE_PACK", false);
  bool AllowM042 = envFlag("FUZZX_ALLOW_M042_OR_LSHR_ZERO", false);
  bool AllowM043 = envFlag("FUZZX_ALLOW_M043_SELF_XOR", false);
  bool AllowM044 = envFlag("FUZZX_ALLOW_M044_V4I32_SELF_AND", false);
  bool AllowM045 = envFlag("FUZZX_ALLOW_M045_UREM_OR_ONE", false);
  bool AllowM046 = envFlag("FUZZX_ALLOW_M046_V4I16_CTTZ", false);
  bool AllowM047 = envFlag("FUZZX_ALLOW_M047_V8I8_SHL", false);
  bool AllowM048 = envFlag("FUZZX_ALLOW_M048_V8I8_UADD_SAT", false);
  bool AllowM049 =
      envFlag("FUZZX_ALLOW_M049_VECTOR_FSHL", false) ||
      envFlag("FUZZX_ALLOW_M049_VECTOR_FSHL_ZERO", false);
  bool AllowM050 = envFlag("FUZZX_ALLOW_M050_AND_SUB_ZERO", false);
  bool AllowM051 = envFlag("FUZZX_ALLOW_M051_VECTOR_FSHR_LOOP", false);
  bool AllowM052 = envFlag("FUZZX_ALLOW_M052_TERNARY_BLEND_SHIFT", false);
  bool AllowC001 = envFlag("FUZZX_ALLOW_C001_SUDOT_ISEL_ICE", false);
  bool AllowC002 = envFlag("FUZZX_ALLOW_C002_FMA_LEGACY_ISEL_ICE", false);
  Function *Kernel = findIRKernel(M);
  if (!Kernel)
    return false;
  raw_null_ostream NullOS;
  if (verifyModule(M, &NullOS))
    return false;
  for (Function &F : M) {
    if (&F == Kernel) {
      if (F.empty())
        return false;
      if (!validateMemoryShape(F))
        return false;
      for (BasicBlock &BB : F)
        for (Instruction &I : BB)
          if (!isAllowedIRInstruction(I) ||
              !isValidVectorInstruction(I) ||
              !isValidAggregateInstruction(I) ||
              !isValidFPConversionInstruction(I) ||
              !isValidLoopControlInstruction(I) ||
              (!AllowM001 && triggersM001AShrI16ZExt(I)) ||
              (!AllowM015M016 && triggersM015M016ScalarFshl(I)) ||
              (!AllowM026 && triggersM026UMaxXorAnd(I)) ||
              (!AllowM027 && triggersM027XorAndOr(I)) ||
              (!AllowM028 && triggersM028UMaxAndNot(I)) ||
              (!AllowM029 && triggersM029FshlSelectPhi(I)) ||
              (!AllowM030 && triggersM030CtlzShlOrBitop3(I)) ||
              (!AllowM031 && triggersM031VectorOrExtractSub(I)) ||
              (!AllowM032 && triggersM032LoopVectorSelect(I)) ||
              (!AllowM035 && triggersM035WaveReduceXor(I)) ||
              (!AllowM036 && triggersM036WaveReduceAdd(I)) ||
              (!AllowM039 && triggersM039SExtI8HighBytePack(I)) ||
              (!AllowM040 && triggersM040SignedDivRem24(I)) ||
              (!AllowM041 && triggersM041AShrHighBytePack(I)) ||
              (!AllowM042 && triggersM042OrLShrZero(I)) ||
              (!AllowM043 && triggersM043SelfXor(I)) ||
              (!AllowM044 && triggersM044V4I32SelfAnd(I)) ||
              (!AllowM045 && triggersM045URemOrOne(I)) ||
              (!AllowM046 && triggersM046V4I16Cttz(I)) ||
              (!AllowM047 && triggersM047V8I8Shl(I)) ||
              (!AllowM048 && triggersM048V8I8UAddSat(I)) ||
              (!AllowM049 && triggersM049VectorFshl(I)) ||
              (!AllowM050 && triggersM050AndSubZero(I)) ||
              (!AllowM051 && triggersM051VectorFshr(I)) ||
              (!AllowM052 && triggersM052TernaryBlendShift(I)) ||
              (!AllowC001 && triggersC001SUDotISELICE(I)) ||
              (!AllowC002 && triggersC002FMALegacyISELICE(I)))
            return false;
      continue;
    }
    if (!F.isDeclaration() || !F.isIntrinsic())
      return false;
  }
  return true;
}

std::unique_ptr<Module> parseIRCorpusModule(const uint8_t *Data, size_t Size,
                                            LLVMContext &Ctx, StringRef CPU,
                                            bool *Valid = nullptr) {
  if (Valid)
    *Valid = false;
  if (Size == 0)
    return createIRSkeletonModule(Ctx, CPU);
  StringRef Buffer(reinterpret_cast<const char *>(Data), Size);
  MemoryBufferRef MemBuf(Buffer, "fuzzx-amdgpu-ir-bitcode");
  Expected<std::unique_ptr<Module>> Parsed = parseBitcodeFile(MemBuf, Ctx);
  if (!Parsed) {
    consumeError(Parsed.takeError());
    return createIRSkeletonModule(Ctx, CPU);
  }
  std::unique_ptr<Module> M = std::move(*Parsed);
  scrubPoisonAnnotations(*M);
  if (!validateIRCorpusModule(*M))
    return createIRSkeletonModule(Ctx, CPU);
  if (Valid)
    *Valid = true;
  return M;
}

StoreInst *findIRResultStore(Function &F) {
  StoreInst *Result = nullptr;
  for (BasicBlock &BB : F)
    for (Instruction &I : BB)
      if (auto *Store = dyn_cast<StoreInst>(&I))
        Result = Store;
  return Result;
}

SmallVector<Value *, 32> i32ValuesBefore(Instruction *InsertPt) {
  SmallVector<Value *, 32> Values;
  Function *F = InsertPt->getFunction();
  Type *I32 = Type::getInt32Ty(F->getContext());
  for (Argument &Arg : F->args())
    if (Arg.getType() == I32)
      Values.push_back(&Arg);
  DominatorTree DT(*F);
  for (BasicBlock &BB : *F) {
    for (Instruction &I : BB) {
      if (&I != InsertPt && I.getType() == I32 && DT.dominates(&I, InsertPt))
        Values.push_back(&I);
    }
  }
  return Values;
}

Value *chooseI32Value(Instruction *InsertPt, std::minstd_rand &Gen) {
  LLVMContext &Ctx = InsertPt->getContext();
  SmallVector<Value *, 32> Values = i32ValuesBefore(InsertPt);
  if (!Values.empty() && (Gen() % 3) != 0)
    return Values[Gen() % Values.size()];
  return interestingI32(Ctx, Gen);
}

ICmpInst::Predicate randomICmpPredicate(std::minstd_rand &Gen) {
  static constexpr std::array<ICmpInst::Predicate, 10> Predicates = {
      ICmpInst::ICMP_EQ,  ICmpInst::ICMP_NE,  ICmpInst::ICMP_UGT,
      ICmpInst::ICMP_UGE, ICmpInst::ICMP_ULT, ICmpInst::ICMP_ULE,
      ICmpInst::ICMP_SGT, ICmpInst::ICMP_SGE, ICmpInst::ICMP_SLT,
      ICmpInst::ICMP_SLE};
  return Predicates[Gen() % Predicates.size()];
}

Value *extendI32ToI64(IRBuilder<NoFolder> &B, Value *V,
                      std::minstd_rand &Gen) {
  Type *I64 = Type::getInt64Ty(V->getContext());
  if ((Gen() % 2) == 0)
    return B.CreateZExt(V, I64, "fuzz.zext.i64");
  return B.CreateSExt(V, I64, "fuzz.sext.i64");
}

Value *emitRandomI64Instruction(IRBuilder<NoFolder> &B, Module &M, Value *A,
                                Value *Bv, std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Value *A64 = extendI32ToI64(B, A, Gen);
  Value *B64 = extendI32ToI64(B, Bv, Gen);
  Value *Result = nullptr;
  switch (Gen() % 28) {
  case 0:
    Result = B.CreateAdd(A64, B64, "fuzz.i64.add");
    break;
  case 1:
    Result = B.CreateSub(A64, B64, "fuzz.i64.sub");
    break;
  case 2:
    Result = B.CreateMul(A64, B64, "fuzz.i64.mul");
    break;
  case 3:
    Result = B.CreateXor(A64, B64, "fuzz.i64.xor");
    break;
  case 4:
    Result = B.CreateAnd(A64, B64, "fuzz.i64.and");
    break;
  case 5:
    Result = B.CreateOr(A64, B64, "fuzz.i64.or");
    break;
  case 6:
    Result = B.CreateShl(A64, ConstantInt::get(I64, Gen() & 63u),
                         "fuzz.i64.shl");
    break;
  case 7:
    Result = B.CreateLShr(A64, ConstantInt::get(I64, Gen() & 63u),
                          "fuzz.i64.lshr");
    break;
  case 8:
    Result = B.CreateAShr(A64, ConstantInt::get(I64, Gen() & 63u),
                          "fuzz.i64.ashr");
    break;
  case 9:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctpop, {I64}), {A64},
        "fuzz.i64.ctpop");
    break;
  case 10:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bitreverse, {I64}),
        {A64}, "fuzz.i64.bitreverse");
    break;
  case 11:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bswap, {I64}), {A64},
        "fuzz.i64.bswap");
    break;
  case 12:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctlz, {I64}),
        {A64, ConstantInt::getFalse(Ctx)}, "fuzz.i64.ctlz");
    break;
  case 13:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::cttz, {I64}),
        {A64, ConstantInt::getFalse(Ctx)}, "fuzz.i64.cttz");
    break;
  case 14:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umin, {I64}),
        {A64, B64}, "fuzz.i64.umin");
    break;
  case 15:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umax, {I64}),
        {A64, B64}, "fuzz.i64.umax");
    break;
  case 16:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smin, {I64}),
        {A64, B64}, "fuzz.i64.smin");
    break;
  case 17:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smax, {I64}),
        {A64, B64}, "fuzz.i64.smax");
    break;
  case 18:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::uadd_sat, {I64}),
        {A64, B64}, "fuzz.i64.uadd_sat");
    break;
  case 19:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::usub_sat, {I64}),
        {A64, B64}, "fuzz.i64.usub_sat");
    break;
  case 20:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sadd_sat, {I64}),
        {A64, B64}, "fuzz.i64.sadd_sat");
    break;
  case 21:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ssub_sat, {I64}),
        {A64, B64}, "fuzz.i64.ssub_sat");
    break;
  case 22:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I64}),
        {A64, B64, ConstantInt::get(I64, Gen() & 63u)}, "fuzz.i64.fshl");
    break;
  case 23:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I64}),
        {A64, B64, ConstantInt::get(I64, Gen() & 63u)}, "fuzz.i64.fshr");
    break;
  case 24:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I64}),
        {A64, B64, B.CreateAnd(B64, ConstantInt::get(I64, 63),
                               "fuzz.i64.shift")},
        "fuzz.i64.fshl.dyn");
    break;
  case 25:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I64}),
        {A64, B64, B.CreateAnd(A64, ConstantInt::get(I64, 63),
                               "fuzz.i64.shift")},
        "fuzz.i64.fshr.dyn");
    break;
  case 26:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::abs, {I64}),
        {A64, ConstantInt::getFalse(Ctx)}, "fuzz.i64.abs");
    break;
  default: {
    Value *Cmp = B.CreateICmp(randomICmpPredicate(Gen), A64, B64,
                              "fuzz.i64.cmp");
    Result = B.CreateSelect(Cmp, A64, B64, "fuzz.i64.select");
    break;
  }
  }
  return B.CreateTrunc(Result, I32, "fuzz.trunc.i64");
}

Constant *randomShiftVector(LLVMContext &Ctx, unsigned Lanes,
                            std::minstd_rand &Gen) {
  SmallVector<Constant *, 4> Elements;
  for (unsigned I = 0; I != Lanes; ++I)
    Elements.push_back(ci32(Ctx, Gen() & 31u));
  return ConstantVector::get(Elements);
}

Constant *randomShiftVector(LLVMContext &Ctx, Type *ElemTy, unsigned Lanes,
                            unsigned Width, std::minstd_rand &Gen) {
  SmallVector<Constant *, 8> Elements;
  for (unsigned I = 0; I != Lanes; ++I)
    Elements.push_back(ConstantInt::get(ElemTy, Gen() % Width));
  return ConstantVector::get(Elements);
}

Value *emitVectorBuild(IRBuilder<NoFolder> &B, Type *VecTy,
                       ArrayRef<Value *> Elements) {
  Value *Result = Constant::getNullValue(VecTy);
  LLVMContext &Ctx = VecTy->getContext();
  for (unsigned I = 0, E = Elements.size(); I != E; ++I)
    Result = B.CreateInsertElement(Result, Elements[I], ci32(Ctx, I),
                                   "fuzz.vec.ins");
  return Result;
}

SmallVector<int, 8> randomShuffleMask(unsigned Lanes, std::minstd_rand &Gen) {
  SmallVector<int, 8> Mask;
  Mask.reserve(Lanes);
  for (unsigned I = 0; I != Lanes; ++I)
    Mask.push_back(Gen() % (2 * Lanes));
  return Mask;
}

Value *emitRandomVectorIntrinsic(IRBuilder<NoFolder> &B, Module &M, Type *VecTy,
                                 Value *VA, Value *VB,
                                 std::minstd_rand &Gen,
                                 StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  auto *VT = cast<FixedVectorType>(VecTy);
  auto *ElemTy = cast<IntegerType>(VT->getElementType());
  unsigned Lanes = VT->getNumElements();
  unsigned Width = ElemTy->getBitWidth();
  bool AllowByteSwap = ElemTy->getBitWidth() >= 16;
  bool AllowVectorFshl =
      envFlag("FUZZX_ALLOW_M049_VECTOR_FSHL", false) ||
      envFlag("FUZZX_ALLOW_M049_VECTOR_FSHL_ZERO", false);
  bool AllowVectorFshr = envFlag("FUZZX_ALLOW_M051_VECTOR_FSHR_LOOP", false);
  unsigned Choice = Gen() % (AllowByteSwap ? 16 : 15);
  if (!AllowByteSwap && Choice >= 2)
    ++Choice;

  Intrinsic::ID ID;
  switch (Choice) {
  case 0:
    ID = Intrinsic::ctpop;
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                        {VA}, Twine(NamePrefix) + ".ctpop");
  case 1:
    ID = Intrinsic::bitreverse;
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                        {VA}, Twine(NamePrefix) + ".bitreverse");
  case 2:
    ID = Intrinsic::bswap;
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                        {VA}, Twine(NamePrefix) + ".bswap");
  case 3:
    ID = Intrinsic::ctlz;
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                        {VA, ConstantInt::getFalse(Ctx)},
                        Twine(NamePrefix) + ".ctlz");
  case 4:
    ID = Intrinsic::cttz;
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                        {VA, ConstantInt::getFalse(Ctx)},
                        Twine(NamePrefix) + ".cttz");
  case 5:
    ID = Intrinsic::abs;
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                        {VA, ConstantInt::getFalse(Ctx)},
                        Twine(NamePrefix) + ".abs");
  case 6:
    ID = Intrinsic::umin;
    break;
  case 7:
    ID = Intrinsic::umax;
    break;
  case 8:
    ID = Intrinsic::smin;
    break;
  case 9:
    ID = Intrinsic::smax;
    break;
  case 10:
    ID = Intrinsic::uadd_sat;
    break;
  case 11:
    ID = Intrinsic::usub_sat;
    break;
  case 12:
    ID = Intrinsic::sadd_sat;
    break;
  case 13:
    ID = Intrinsic::ssub_sat;
    break;
  case 14:
    if (AllowVectorFshl)
      ID = Intrinsic::fshl;
    else if (AllowVectorFshr)
      ID = Intrinsic::fshr;
    else {
      ID = Intrinsic::umax;
      break;
    }
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
        {VA, VB, randomShiftVector(Ctx, ElemTy, Lanes, Width, Gen)},
        Twine(NamePrefix) + (ID == Intrinsic::fshl ? ".fshl" : ".fshr"));
  default:
    if (!AllowVectorFshr) {
      ID = Intrinsic::smax;
      break;
    }
    ID = Intrinsic::fshr;
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                        {VA, VB, randomShiftVector(Ctx, ElemTy, Lanes, Width,
                                                   Gen)},
                        Twine(NamePrefix) + ".fshr");
  }

  return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {VecTy}),
                      {VA, VB}, Twine(NamePrefix) + ".binary");
}

Value *emitRandomVectorInstruction(IRBuilder<NoFolder> &B, Module &M, Value *A,
                                   Value *Bv, std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  unsigned Lanes = (Gen() % 2) == 0 ? 2 : 4;
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  SmallVector<Value *, 4> AElements;
  SmallVector<Value *, 4> BElements;
  for (unsigned I = 0; I != Lanes; ++I) {
    AElements.push_back((I % 2) == 0 ? A : Bv);
    if ((Gen() % 3) == 0)
      BElements.push_back(interestingI32(Ctx, Gen));
    else
      BElements.push_back((I % 2) == 0 ? Bv : A);
  }

  Value *VA = emitVectorBuild(B, VecTy, AElements);
  Value *VB = emitVectorBuild(B, VecTy, BElements);
  Value *Result = nullptr;
  switch (Gen() % 21) {
  case 0:
    Result = B.CreateAdd(VA, VB, "fuzz.vec.add");
    break;
  case 1:
    Result = B.CreateSub(VA, VB, "fuzz.vec.sub");
    break;
  case 2:
    Result = B.CreateMul(VA, VB, "fuzz.vec.mul");
    break;
  case 3:
    Result = B.CreateXor(VA, VB, "fuzz.vec.xor");
    break;
  case 4:
    Result = B.CreateAnd(VA, VB, "fuzz.vec.and");
    break;
  case 5:
    Result = B.CreateOr(VA, VB, "fuzz.vec.or");
    break;
  case 6:
    Result = B.CreateShl(VA, randomShiftVector(Ctx, Lanes, Gen),
                         "fuzz.vec.shl");
    break;
  case 7:
    Result = B.CreateLShr(VA, randomShiftVector(Ctx, Lanes, Gen),
                          "fuzz.vec.lshr");
    break;
  case 8:
    Result = B.CreateAShr(VA, randomShiftVector(Ctx, Lanes, Gen),
                          "fuzz.vec.ashr");
    break;
  case 9:
  case 10:
  case 11:
  case 12:
  case 13:
  case 14:
  case 15:
    Result = emitRandomVectorIntrinsic(B, M, VecTy, VA, VB, Gen, "fuzz.vec");
    break;
  case 16:
  case 17:
    Result = B.CreateShuffleVector(VA, VB, randomShuffleMask(Lanes, Gen),
                                   "fuzz.vec.shuffle");
    break;
  default: {
    Value *Cmp = B.CreateICmp(randomICmpPredicate(Gen), VA, VB,
                              "fuzz.vec.cmp");
    Result = B.CreateSelect(Cmp, VA, VB, "fuzz.vec.select");
    break;
  }
  }

  unsigned Lane0 = Gen() % Lanes;
  unsigned Lane1 = Gen() % Lanes;
  Value *E0 = B.CreateExtractElement(Result, ci32(Ctx, Lane0), "fuzz.vec.ext");
  Value *E1 = B.CreateExtractElement(Result, ci32(Ctx, Lane1), "fuzz.vec.ext");
  switch (Gen() % 4) {
  case 0:
    return B.CreateXor(E0, E1, "fuzz.vec.reduce.xor");
  case 1:
    return B.CreateAdd(E0, E1, "fuzz.vec.reduce.add");
  case 2:
    return B.CreateOr(E0, E1, "fuzz.vec.reduce.or");
  default:
    return B.CreateSub(E0, E1, "fuzz.vec.reduce.sub");
  }
}

Value *emitRandomNarrowVectorInstruction(IRBuilder<NoFolder> &B, Module &M,
                                         Value *A, Value *Bv,
                                         std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  bool UseI8 = (Gen() % 2) == 0;
  Type *ElemTy = UseI8 ? Type::getInt8Ty(Ctx) : Type::getInt16Ty(Ctx);
  unsigned Width = UseI8 ? 8 : 16;
  unsigned Lanes = (Gen() % 2) == 0 ? 4 : 8;
  auto *VecTy = FixedVectorType::get(ElemTy, Lanes);
  SmallVector<Value *, 8> AElements;
  SmallVector<Value *, 8> BElements;
  for (unsigned I = 0; I != Lanes; ++I) {
    Value *ASeed = (I % 2) == 0 ? A : Bv;
    Value *BSeed = nullptr;
    if ((Gen() % 3) == 0)
      BSeed = interestingI32(Ctx, Gen);
    else
      BSeed = (I % 2) == 0 ? Bv : A;
    AElements.push_back(
        B.CreateTrunc(ASeed, ElemTy, "fuzz.vec.narrow.trunc"));
    BElements.push_back(
        B.CreateTrunc(BSeed, ElemTy, "fuzz.vec.narrow.trunc"));
  }

  Value *VA = emitVectorBuild(B, VecTy, AElements);
  Value *VB = emitVectorBuild(B, VecTy, BElements);
  Value *Result = nullptr;
  switch (Gen() % 20) {
  case 0:
    Result = B.CreateAdd(VA, VB, "fuzz.vec.narrow.add");
    break;
  case 1:
    Result = B.CreateSub(VA, VB, "fuzz.vec.narrow.sub");
    break;
  case 2:
    Result = B.CreateMul(VA, VB, "fuzz.vec.narrow.mul");
    break;
  case 3:
    Result = B.CreateXor(VA, VB, "fuzz.vec.narrow.xor");
    break;
  case 4:
    Result = B.CreateAnd(VA, VB, "fuzz.vec.narrow.and");
    break;
  case 5:
    Result = B.CreateOr(VA, VB, "fuzz.vec.narrow.or");
    break;
  case 6:
    Result = B.CreateShl(VA, randomShiftVector(Ctx, ElemTy, Lanes, Width, Gen),
                         "fuzz.vec.narrow.shl");
    break;
  case 7:
    Result = B.CreateLShr(VA, randomShiftVector(Ctx, ElemTy, Lanes, Width, Gen),
                          "fuzz.vec.narrow.lshr");
    break;
  case 8:
    Result = B.CreateAShr(VA, randomShiftVector(Ctx, ElemTy, Lanes, Width, Gen),
                          "fuzz.vec.narrow.ashr");
    break;
  case 9:
  case 10:
  case 11:
  case 12:
  case 13:
  case 14:
    Result = emitRandomVectorIntrinsic(B, M, VecTy, VA, VB, Gen,
                                       "fuzz.vec.narrow");
    break;
  case 15:
  case 16:
    Result = B.CreateShuffleVector(VA, VB, randomShuffleMask(Lanes, Gen),
                                   "fuzz.vec.narrow.shuffle");
    break;
  default: {
    Value *Cmp = B.CreateICmp(randomICmpPredicate(Gen), VA, VB,
                              "fuzz.vec.narrow.cmp");
    Result = B.CreateSelect(Cmp, VA, VB, "fuzz.vec.narrow.select");
    break;
  }
  }

  unsigned Lane0 = Gen() % Lanes;
  unsigned Lane1 = Gen() % Lanes;
  Value *E0 =
      B.CreateExtractElement(Result, ci32(Ctx, Lane0), "fuzz.vec.narrow.ext");
  Value *E1 =
      B.CreateExtractElement(Result, ci32(Ctx, Lane1), "fuzz.vec.narrow.ext");
  Value *E0I32 = (Gen() % 2) == 0
                     ? B.CreateZExt(E0, I32, "fuzz.vec.narrow.zext")
                     : B.CreateSExt(E0, I32, "fuzz.vec.narrow.sext");
  Value *E1I32 = (Gen() % 2) == 0
                     ? B.CreateZExt(E1, I32, "fuzz.vec.narrow.zext")
                     : B.CreateSExt(E1, I32, "fuzz.vec.narrow.sext");
  switch (Gen() % 4) {
  case 0:
    return B.CreateXor(E0I32, E1I32, "fuzz.vec.narrow.reduce.xor");
  case 1:
    return B.CreateAdd(E0I32, E1I32, "fuzz.vec.narrow.reduce.add");
  case 2:
    return B.CreateOr(E0I32, E1I32, "fuzz.vec.narrow.reduce.or");
  default:
    return B.CreateSub(E0I32, E1I32, "fuzz.vec.narrow.reduce.sub");
  }
}

FCmpInst::Predicate randomFCmpPredicate(std::minstd_rand &Gen);

Value *emitRandomVectorFPInstruction(IRBuilder<NoFolder> &B, Module &M,
                                     Value *A, Value *Bv,
                                     std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  unsigned Lanes = (Gen() % 2) == 0 ? 2 : 4;
  auto *VecTy = FixedVectorType::get(F32, Lanes);
  SmallVector<Value *, 4> AElements;
  SmallVector<Value *, 4> BElements;
  for (unsigned I = 0; I != Lanes; ++I) {
    Value *ASeed = (I % 2) == 0 ? A : Bv;
    Value *BSeed = nullptr;
    if ((Gen() % 3) == 0)
      BSeed = interestingI32(Ctx, Gen);
    else
      BSeed = (I % 2) == 0 ? Bv : A;
    Value *AMasked =
        B.CreateAnd(ASeed, ci32(Ctx, 255), "fuzz.vec.fp.mask");
    Value *BMasked =
        B.CreateAnd(BSeed, ci32(Ctx, 255), "fuzz.vec.fp.mask");
    AElements.push_back(B.CreateUIToFP(AMasked, F32, "fuzz.vec.fp.uitofp"));
    BElements.push_back(B.CreateUIToFP(BMasked, F32, "fuzz.vec.fp.uitofp"));
  }

  Value *VA = emitVectorBuild(B, VecTy, AElements);
  Value *VB = emitVectorBuild(B, VecTy, BElements);
  Value *Result = nullptr;
  switch (Gen() % 5) {
  case 0:
    Result = B.CreateFAdd(VA, VB, "fuzz.vec.fp.fadd");
    break;
  case 1:
    Result = B.CreateFMul(VA, VB, "fuzz.vec.fp.fmul");
    break;
  case 2: {
    Value *Product = B.CreateFMul(VA, VB, "fuzz.vec.fp.fmul");
    Result = B.CreateFAdd(Product, VA, "fuzz.vec.fp.fmaish");
    break;
  }
  default: {
    Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), VA, VB,
                              "fuzz.vec.fp.fcmp");
    Result = B.CreateSelect(Cmp, VA, VB, "fuzz.vec.fp.select");
    break;
  }
  }

  unsigned Lane0 = Gen() % Lanes;
  unsigned Lane1 = Gen() % Lanes;
  Value *F0 = B.CreateExtractElement(Result, ci32(Ctx, Lane0),
                                     "fuzz.vec.fp.ext");
  Value *F1 = B.CreateExtractElement(Result, ci32(Ctx, Lane1),
                                     "fuzz.vec.fp.ext");
  Value *I0 = B.CreateFPToUI(F0, I32, "fuzz.vec.fp.fptoui");
  Value *I1 = B.CreateFPToUI(F1, I32, "fuzz.vec.fp.fptoui");
  switch (Gen() % 4) {
  case 0:
    return B.CreateXor(I0, I1, "fuzz.vec.fp.reduce.xor");
  case 1:
    return B.CreateAdd(I0, I1, "fuzz.vec.fp.reduce.add");
  case 2:
    return B.CreateOr(I0, I1, "fuzz.vec.fp.reduce.or");
  default:
    return B.CreateSub(I0, I1, "fuzz.vec.fp.reduce.sub");
  }
}

Value *emitRandomVectorHalfFPInstruction(IRBuilder<NoFolder> &B, Module &M,
                                         Value *A, Value *Bv,
                                         std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F16 = Type::getHalfTy(Ctx);
  unsigned Lanes = (Gen() % 2) == 0 ? 2 : 4;
  auto *VecTy = FixedVectorType::get(F16, Lanes);
  SmallVector<Value *, 4> AElements;
  SmallVector<Value *, 4> BElements;
  for (unsigned I = 0; I != Lanes; ++I) {
    Value *ASeed = (I % 2) == 0 ? A : Bv;
    Value *BSeed = nullptr;
    if ((Gen() % 3) == 0)
      BSeed = interestingI32(Ctx, Gen);
    else
      BSeed = (I % 2) == 0 ? Bv : A;
    Value *AMasked =
        B.CreateAnd(ASeed, ci32(Ctx, 127), "fuzz.vec.fp16.mask");
    Value *BMasked =
        B.CreateAnd(BSeed, ci32(Ctx, 127), "fuzz.vec.fp16.mask");
    AElements.push_back(B.CreateUIToFP(AMasked, F16, "fuzz.vec.fp16.uitofp"));
    BElements.push_back(B.CreateUIToFP(BMasked, F16, "fuzz.vec.fp16.uitofp"));
  }

  Value *VA = emitVectorBuild(B, VecTy, AElements);
  Value *VB = emitVectorBuild(B, VecTy, BElements);
  Value *Result = nullptr;
  switch (Gen() % 5) {
  case 0:
    Result = B.CreateFAdd(VA, VB, "fuzz.vec.fp16.fadd");
    break;
  case 1:
    Result = B.CreateFMul(VA, VB, "fuzz.vec.fp16.fmul");
    break;
  case 2: {
    Value *Product = B.CreateFMul(VA, VB, "fuzz.vec.fp16.fmul");
    Result = B.CreateFAdd(Product, VA, "fuzz.vec.fp16.fmaish");
    break;
  }
  case 3: {
    Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), VA, VB,
                              "fuzz.vec.fp16.fcmp");
    Result = B.CreateSelect(Cmp, VA, VB, "fuzz.vec.fp16.select");
    break;
  }
  default: {
    Value *Cmp = B.CreateFCmpOGE(VA, VB, "fuzz.vec.fp16.oge");
    Value *Hi = B.CreateSelect(Cmp, VA, VB, "fuzz.vec.fp16.hi");
    Value *Lo = B.CreateSelect(Cmp, VB, VA, "fuzz.vec.fp16.lo");
    Result = B.CreateFSub(Hi, Lo, "fuzz.vec.fp16.fsub");
    break;
  }
  }

  unsigned Lane0 = Gen() % Lanes;
  unsigned Lane1 = Gen() % Lanes;
  Value *F0 =
      B.CreateExtractElement(Result, ci32(Ctx, Lane0), "fuzz.vec.fp16.ext");
  Value *F1 =
      B.CreateExtractElement(Result, ci32(Ctx, Lane1), "fuzz.vec.fp16.ext");
  Value *I0 = B.CreateFPToUI(F0, I32, "fuzz.vec.fp16.fptoui");
  Value *I1 = B.CreateFPToUI(F1, I32, "fuzz.vec.fp16.fptoui");
  switch (Gen() % 4) {
  case 0:
    return B.CreateXor(I0, I1, "fuzz.vec.fp16.reduce.xor");
  case 1:
    return B.CreateAdd(I0, I1, "fuzz.vec.fp16.reduce.add");
  case 2:
    return B.CreateOr(I0, I1, "fuzz.vec.fp16.reduce.or");
  default:
    return B.CreateSub(I0, I1, "fuzz.vec.fp16.reduce.sub");
  }
}

Value *emitRandomBoolI32Instruction(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                    std::minstd_rand &Gen,
                                    StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *P =
      B.CreateICmp(randomICmpPredicate(Gen), A, Bv, Twine(NamePrefix) + ".cmp0");
  Value *Q =
      B.CreateICmp(randomICmpPredicate(Gen), Bv,
                   (Gen() % 2) == 0 ? A : interestingI32(Ctx, Gen),
                   Twine(NamePrefix) + ".cmp1");
  Value *R = nullptr;
  switch (Gen() % 5) {
  case 0:
    R = B.CreateAnd(P, Q, Twine(NamePrefix) + ".and");
    break;
  case 1:
    R = B.CreateOr(P, Q, Twine(NamePrefix) + ".or");
    break;
  case 2:
    R = B.CreateXor(P, Q, Twine(NamePrefix) + ".xor");
    break;
  case 3:
    R = B.CreateSelect(P, Q, ConstantInt::getFalse(Ctx),
                       Twine(NamePrefix) + ".select");
    break;
  default:
    R = B.CreateXor(P, ConstantInt::getTrue(Ctx), Twine(NamePrefix) + ".not");
    break;
  }

  Value *Z = B.CreateZExt(R, I32, Twine(NamePrefix) + ".zext");
  switch (Gen() % 4) {
  case 0:
    return Z;
  case 1:
    return B.CreateSelect(R, A, Bv, Twine(NamePrefix) + ".i32.select");
  case 2:
    return B.CreateXor(A, Z, Twine(NamePrefix) + ".xor.i32");
  default:
    return B.CreateSub(A, Z, Twine(NamePrefix) + ".sub.i32");
  }
}

Value *buildPredicateMask(IRBuilder<NoFolder> &B, Value *Pred,
                          std::minstd_rand &Gen, const Twine &NamePrefix) {
  LLVMContext &Ctx = Pred->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  if ((Gen() % 2) == 0)
    return B.CreateSExt(Pred, I32, Twine(NamePrefix) + ".sext");
  return B.CreateSelect(Pred, ci32(Ctx, 0xffffffffu), ci32(Ctx, 0),
                        Twine(NamePrefix) + ".select");
}

Value *emitRandomPredicateMaskIdiom(IRBuilder<NoFolder> &B, Value *A,
                                    Value *Bv, std::minstd_rand &Gen,
                                    StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *Cmp =
      B.CreateICmp(randomICmpPredicate(Gen), A, Bv, Twine(NamePrefix) + ".cmp");
  Value *Mask = buildPredicateMask(B, Cmp, Gen, Twine(NamePrefix) + ".mask");
  Value *NotMask =
      B.CreateXor(Mask, ci32(Ctx, 0xffffffffu), Twine(NamePrefix) + ".not");

  switch (Gen() % 7) {
  case 0: {
    Value *LHS = B.CreateAnd(A, Mask, Twine(NamePrefix) + ".blend.a");
    Value *RHS = B.CreateAnd(Bv, NotMask, Twine(NamePrefix) + ".blend.b");
    return B.CreateOr(LHS, RHS, Twine(NamePrefix) + ".blend.or");
  }
  case 1: {
    Value *LHS = B.CreateAnd(A, NotMask, Twine(NamePrefix) + ".blend.a");
    Value *RHS = B.CreateAnd(Bv, Mask, Twine(NamePrefix) + ".blend.b");
    return B.CreateAdd(LHS, RHS, Twine(NamePrefix) + ".blend.add");
  }
  case 2:
    return B.CreateAnd(A, Mask, Twine(NamePrefix) + ".clear");
  case 3:
    return B.CreateOr(A, Mask, Twine(NamePrefix) + ".set");
  case 4:
    return B.CreateXor(A, Mask, Twine(NamePrefix) + ".toggle");
  case 5: {
    Value *Sign = B.CreateAShr(A, ci32(Ctx, 31), Twine(NamePrefix) + ".sign");
    Value *Flipped = B.CreateXor(A, Sign, Twine(NamePrefix) + ".abs.flip");
    Value *Abs = B.CreateSub(Flipped, Sign, Twine(NamePrefix) + ".abs");
    return B.CreateSelect(Cmp, Abs, A, Twine(NamePrefix) + ".abs.select");
  }
  default: {
    Value *Sign = B.CreateAShr(A, ci32(Ctx, 31), Twine(NamePrefix) + ".sign");
    Value *Adjusted = B.CreateSub(B.CreateXor(A, Sign,
                                              Twine(NamePrefix) + ".sign.xor"),
                                  Sign, Twine(NamePrefix) + ".sign.sub");
    return B.CreateXor(Adjusted, Mask, Twine(NamePrefix) + ".sign.mask.xor");
  }
  }
}

Value *emitRandomBitfieldIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                               std::minstd_rand &Gen, StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *ShiftSeed = (Gen() % 2) == 0 ? A : Bv;
  Value *WidthSeed = (Gen() % 2) == 0 ? Bv : interestingI32(Ctx, Gen);
  Value *Shift =
      B.CreateAnd(ShiftSeed, ci32(Ctx, 15), Twine(NamePrefix) + ".shift");
  Value *WidthMinusOne =
      B.CreateAnd(WidthSeed, ci32(Ctx, 15), Twine(NamePrefix) + ".width.m1");
  Value *Width =
      B.CreateAdd(WidthMinusOne, ci32(Ctx, 1), Twine(NamePrefix) + ".width");
  Value *InvWidth =
      B.CreateAnd(B.CreateSub(ci32(Ctx, 32), Width,
                              Twine(NamePrefix) + ".invwidth.raw"),
                  ci32(Ctx, 31), Twine(NamePrefix) + ".invwidth");
  Value *Mask =
      B.CreateLShr(ci32(Ctx, 0xffffffffu), InvWidth,
                   Twine(NamePrefix) + ".mask");
  Value *Shifted = B.CreateLShr(A, Shift, Twine(NamePrefix) + ".shifted");
  Value *Extracted =
      B.CreateAnd(Shifted, Mask, Twine(NamePrefix) + ".extracted");

  switch (Gen() % 6) {
  case 0:
    return Extracted;
  case 1: {
    Value *Left =
        B.CreateShl(Extracted, InvWidth, Twine(NamePrefix) + ".sext.left");
    return B.CreateAShr(Left, InvWidth, Twine(NamePrefix) + ".sext");
  }
  case 2: {
    Value *FieldMask =
        B.CreateShl(Mask, Shift, Twine(NamePrefix) + ".fieldmask");
    Value *NotFieldMask =
        B.CreateXor(FieldMask, ci32(Ctx, 0xffffffffu),
                    Twine(NamePrefix) + ".notfieldmask");
    Value *Cleared = B.CreateAnd(A, NotFieldMask, Twine(NamePrefix) + ".clear");
    Value *Payload =
        B.CreateAnd(Bv, Mask, Twine(NamePrefix) + ".payload.masked");
    Value *Inserted =
        B.CreateShl(Payload, Shift, Twine(NamePrefix) + ".payload.shifted");
    return B.CreateOr(Cleared, Inserted, Twine(NamePrefix) + ".insert");
  }
  case 3: {
    Value *FieldMask =
        B.CreateShl(Mask, Shift, Twine(NamePrefix) + ".fieldmask");
    Value *NotFieldMask =
        B.CreateXor(FieldMask, ci32(Ctx, 0xffffffffu),
                    Twine(NamePrefix) + ".notfieldmask");
    Value *Cleared = B.CreateAnd(A, NotFieldMask, Twine(NamePrefix) + ".clear");
    Value *Payload =
        B.CreateShl(B.CreateAnd(Bv, Mask, Twine(NamePrefix) + ".payload.masked"),
                    Shift, Twine(NamePrefix) + ".payload.shifted");
    Value *PayloadField =
        B.CreateAnd(Payload, FieldMask, Twine(NamePrefix) + ".payload.field");
    return B.CreateAdd(Cleared, PayloadField,
                       Twine(NamePrefix) + ".insert.add");
  }
  case 4:
    return B.CreateOr(Extracted, B.CreateShl(Mask, Shift,
                                             Twine(NamePrefix) + ".mask.shift"),
                      Twine(NamePrefix) + ".extract.or.mask");
  default:
    return B.CreateXor(Extracted, B.CreateAnd(Bv, Mask,
                                              Twine(NamePrefix) + ".rhs.mask"),
                       Twine(NamePrefix) + ".extract.xor");
  }
}

Value *emitRandomWideningMulIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                  std::minstd_rand &Gen,
                                  StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);

  Value *AZ = B.CreateZExt(A, I64, Twine(NamePrefix) + ".a.zext");
  Value *BZ = B.CreateZExt(Bv, I64, Twine(NamePrefix) + ".b.zext");
  Value *AS = B.CreateSExt(A, I64, Twine(NamePrefix) + ".a.sext");
  Value *BS = B.CreateSExt(Bv, I64, Twine(NamePrefix) + ".b.sext");

  switch (Gen() % 6) {
  case 0: {
    Value *Product = B.CreateMul(AZ, BZ, Twine(NamePrefix) + ".umul");
    Value *High = B.CreateLShr(Product, ConstantInt::get(I64, 32),
                               Twine(NamePrefix) + ".uhi");
    return B.CreateTrunc(High, I32, Twine(NamePrefix) + ".uhi.i32");
  }
  case 1: {
    Value *Product = B.CreateMul(AS, BS, Twine(NamePrefix) + ".smul");
    Value *High = B.CreateAShr(Product, ConstantInt::get(I64, 32),
                               Twine(NamePrefix) + ".shi");
    return B.CreateTrunc(High, I32, Twine(NamePrefix) + ".shi.i32");
  }
  case 2: {
    Value *Product = B.CreateMul(AZ, BZ, Twine(NamePrefix) + ".umul");
    Value *Low = B.CreateTrunc(Product, I32, Twine(NamePrefix) + ".ulo.i32");
    Value *High = B.CreateTrunc(
        B.CreateLShr(Product, ConstantInt::get(I64, 32),
                     Twine(NamePrefix) + ".uhi"),
        I32, Twine(NamePrefix) + ".uhi.i32");
    return B.CreateXor(Low, High, Twine(NamePrefix) + ".u.xor");
  }
  case 3: {
    Value *Product = B.CreateMul(AS, BS, Twine(NamePrefix) + ".smul");
    Value *Low = B.CreateTrunc(Product, I32, Twine(NamePrefix) + ".slo.i32");
    Value *High = B.CreateTrunc(
        B.CreateAShr(Product, ConstantInt::get(I64, 32),
                     Twine(NamePrefix) + ".shi"),
        I32, Twine(NamePrefix) + ".shi.i32");
    return B.CreateAdd(Low, High, Twine(NamePrefix) + ".s.add");
  }
  case 4: {
    Value *Product = B.CreateMul(AS, BZ, Twine(NamePrefix) + ".mixed.mul");
    Value *High = B.CreateAShr(Product, ConstantInt::get(I64, 32),
                               Twine(NamePrefix) + ".mixed.hi");
    return B.CreateTrunc(High, I32, Twine(NamePrefix) + ".mixed.hi.i32");
  }
  default: {
    Value *AMasked = B.CreateAnd(A, ci32(Ctx, 0xffff),
                                 Twine(NamePrefix) + ".a.masked");
    Value *BMasked = B.CreateAnd(Bv, ci32(Ctx, 0xffff),
                                 Twine(NamePrefix) + ".b.masked");
    Value *Product = B.CreateMul(B.CreateZExt(AMasked, I64,
                                              Twine(NamePrefix) + ".a16.zext"),
                                 B.CreateZExt(BMasked, I64,
                                              Twine(NamePrefix) + ".b16.zext"),
                                 Twine(NamePrefix) + ".u16.mul");
    return B.CreateTrunc(Product, I32, Twine(NamePrefix) + ".u16.lo");
  }
  }
}

Value *extractByteAsI32(IRBuilder<NoFolder> &B, Value *V, unsigned Byte,
                        const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Shifted = V;
  if (Byte != 0)
    Shifted = B.CreateLShr(V, ci32(Ctx, Byte * 8), Name + ".shr");
  return B.CreateZExt(B.CreateTrunc(Shifted, I8, Name + ".trunc"), I32,
                      Name + ".zext");
}

Value *extractSignedByteAsI32(IRBuilder<NoFolder> &B, Value *V, unsigned Byte,
                              const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Shifted = V;
  if (Byte != 0)
    Shifted = B.CreateLShr(V, ci32(Ctx, Byte * 8), Name + ".shr");
  return B.CreateSExt(B.CreateTrunc(Shifted, I8, Name + ".trunc"), I32,
                      Name + ".sext");
}

Value *extractHalfAsI32(IRBuilder<NoFolder> &B, Value *V, unsigned Half,
                        bool Signed, const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Shifted = V;
  if (Half != 0)
    Shifted = B.CreateLShr(V, ci32(Ctx, 16), Name + ".shr");
  Value *Truncated = B.CreateTrunc(Shifted, I16, Name + ".trunc");
  if (Signed)
    return B.CreateSExt(Truncated, I32, Name + ".sext");
  return B.CreateZExt(Truncated, I32, Name + ".zext");
}

Value *packFourBytesAsI32(IRBuilder<NoFolder> &B, ArrayRef<Value *> Bytes,
                          bool UseAdd, const Twine &Name) {
  LLVMContext &Ctx = Bytes[0]->getContext();
  Value *Result = ci32(Ctx, 0);
  for (unsigned I = 0; I != 4; ++I) {
    Value *Lane = B.CreateAnd(Bytes[I], ci32(Ctx, 0xff), Name + ".mask");
    if (I != 0)
      Lane = B.CreateShl(Lane, ci32(Ctx, I * 8), Name + ".shift");
    Result = UseAdd ? B.CreateAdd(Result, Lane, Name + ".add")
                    : B.CreateOr(Result, Lane, Name + ".or");
  }
  return Result;
}

Value *emitRandomPackUnpackIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                 std::minstd_rand &Gen,
                                 StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  auto AByte = [&](unsigned Byte, const Twine &Name) {
    return extractByteAsI32(B, A, Byte, Name);
  };
  auto BByte = [&](unsigned Byte, const Twine &Name) {
    return extractByteAsI32(B, Bv, Byte, Name);
  };

  switch (Gen() % 8) {
  case 0: {
    Value *A0 = AByte(0, Twine(NamePrefix) + ".a0");
    Value *A1 = AByte(1, Twine(NamePrefix) + ".a1");
    Value *A2 = AByte(2, Twine(NamePrefix) + ".a2");
    Value *A3 = AByte(3, Twine(NamePrefix) + ".a3");
    return packFourBytesAsI32(B, {A3, A2, A1, A0}, (Gen() % 2) == 0,
                              Twine(NamePrefix) + ".bswap");
  }
  case 1: {
    Value *A0 = AByte(0, Twine(NamePrefix) + ".a0");
    Value *A1 = AByte(1, Twine(NamePrefix) + ".a1");
    Value *B0 = BByte(0, Twine(NamePrefix) + ".b0");
    Value *B1 = BByte(1, Twine(NamePrefix) + ".b1");
    return packFourBytesAsI32(B, {A0, B0, A1, B1}, (Gen() % 2) == 0,
                              Twine(NamePrefix) + ".interleave.lo");
  }
  case 2: {
    Value *A2 = AByte(2, Twine(NamePrefix) + ".a2");
    Value *A3 = AByte(3, Twine(NamePrefix) + ".a3");
    Value *B2 = BByte(2, Twine(NamePrefix) + ".b2");
    Value *B3 = BByte(3, Twine(NamePrefix) + ".b3");
    return packFourBytesAsI32(B, {A2, B2, A3, B3}, (Gen() % 2) == 0,
                              Twine(NamePrefix) + ".interleave.hi");
  }
  case 3: {
    Value *Lo =
        B.CreateAnd(A, ci32(Ctx, 0xffff), Twine(NamePrefix) + ".lo16");
    Value *Hi =
        B.CreateAnd(Bv, ci32(Ctx, 0xffff), Twine(NamePrefix) + ".hi16");
    Hi = B.CreateShl(Hi, ci32(Ctx, 16), Twine(NamePrefix) + ".hi16.shift");
    return B.CreateOr(Lo, Hi, Twine(NamePrefix) + ".half.merge");
  }
  case 4: {
    Value *A0 = AByte(0, Twine(NamePrefix) + ".a0");
    Value *A2 = AByte(2, Twine(NamePrefix) + ".a2");
    Value *B1 = BByte(1, Twine(NamePrefix) + ".b1");
    Value *B3 = BByte(3, Twine(NamePrefix) + ".b3");
    Value *Packed = packFourBytesAsI32(B, {B3, A0, B1, A2}, false,
                                       Twine(NamePrefix) + ".pack");
    Value *Half = extractHalfAsI32(B, Packed, Gen() & 1u, (Gen() % 2) == 0,
                                   Twine(NamePrefix) + ".half");
    return B.CreateXor(Half, A, Twine(NamePrefix) + ".half.xor");
  }
  case 5: {
    Value *A0 = AByte(0, Twine(NamePrefix) + ".a0");
    Value *A1 = AByte(1, Twine(NamePrefix) + ".a1");
    Value *A2 = AByte(2, Twine(NamePrefix) + ".a2");
    Value *B0 = BByte(0, Twine(NamePrefix) + ".b0");
    Value *B1 = BByte(1, Twine(NamePrefix) + ".b1");
    Value *B2 = BByte(2, Twine(NamePrefix) + ".b2");
    Value *P0 = B.CreateMul(A0, B0, Twine(NamePrefix) + ".byte.mul0");
    Value *P1 = B.CreateMul(A1, B1, Twine(NamePrefix) + ".byte.mul1");
    Value *P2 = B.CreateMul(A2, B2, Twine(NamePrefix) + ".byte.mul2");
    return B.CreateAdd(B.CreateAdd(P0, P1, Twine(NamePrefix) + ".byte.sum01"),
                       P2, Twine(NamePrefix) + ".byte.sum");
  }
  case 6: {
    Value *LoNibbles =
        B.CreateAnd(A, ci32(Ctx, 0x0f0f0f0fu), Twine(NamePrefix) + ".lo.nib");
    Value *HiNibbles =
        B.CreateLShr(B.CreateAnd(Bv, ci32(Ctx, 0xf0f0f0f0u),
                                 Twine(NamePrefix) + ".hi.nib.mask"),
                     ci32(Ctx, 4), Twine(NamePrefix) + ".hi.nib");
    Value *Merged =
        B.CreateOr(LoNibbles, HiNibbles, Twine(NamePrefix) + ".nib.merge");
    Value *E0 = extractByteAsI32(B, Merged, 0, Twine(NamePrefix) + ".nib.e0");
    Value *E2 = extractByteAsI32(B, Merged, 2, Twine(NamePrefix) + ".nib.e2");
    Value *B0 = BByte(0, Twine(NamePrefix) + ".b0");
    Value *B2 = BByte(2, Twine(NamePrefix) + ".b2");
    return packFourBytesAsI32(B, {E2, B0, E0, B2}, true,
                              Twine(NamePrefix) + ".nib.repack");
  }
  default: {
    Value *Lo64 = B.CreateZExt(A, I64, Twine(NamePrefix) + ".lo64");
    Value *Hi64 = B.CreateShl(B.CreateZExt(Bv, I64,
                                           Twine(NamePrefix) + ".hi64"),
                              ConstantInt::get(I64, 32),
                              Twine(NamePrefix) + ".hi64.shift");
    Value *Pair = B.CreateOr(Lo64, Hi64, Twine(NamePrefix) + ".pair");
    Value *Lane =
        B.CreateLShr(Pair, ConstantInt::get(I64, (Gen() & 1u) ? 32 : 0),
                     Twine(NamePrefix) + ".pair.extract");
    Value *Lane32 = B.CreateTrunc(Lane, I32, Twine(NamePrefix) + ".lane32");
    Value *A0 = AByte(0, Twine(NamePrefix) + ".a0");
    Value *B3 = BByte(3, Twine(NamePrefix) + ".b3");
    return B.CreateAdd(Lane32, B.CreateXor(A0, B3,
                                           Twine(NamePrefix) + ".byte.xor"),
                       Twine(NamePrefix) + ".pair.mix");
  }
  }
}

Value *callI32UnaryIntrinsic(IRBuilder<NoFolder> &B, Module &M,
                             Intrinsic::ID ID, Value *V, const Twine &Name) {
  Type *I32 = Type::getInt32Ty(M.getContext());
  if (ID == Intrinsic::ctlz || ID == Intrinsic::cttz)
    return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {I32}),
                        {V, ConstantInt::getFalse(M.getContext())}, Name);
  return B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID, {I32}), {V},
                      Name);
}

Value *emitRandomBitCountIdiom(IRBuilder<NoFolder> &B, Module &M, Value *A,
                               Value *Bv, std::minstd_rand &Gen,
                               StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  switch (Gen() % 8) {
  case 0: {
    Value *Neg = B.CreateSub(ci32(Ctx, 0), A, Twine(NamePrefix) + ".neg");
    Value *LowBit = B.CreateAnd(A, Neg, Twine(NamePrefix) + ".lowbit");
    Value *LowPop = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, LowBit,
                                          Twine(NamePrefix) + ".lowpop");
    return B.CreateXor(LowBit, LowPop, Twine(NamePrefix) + ".lowbit.mix");
  }
  case 1: {
    Value *Dec = B.CreateSub(A, ci32(Ctx, 1), Twine(NamePrefix) + ".dec");
    Value *Cleared = B.CreateAnd(A, Dec, Twine(NamePrefix) + ".clear.lowbit");
    Value *PopA = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, A,
                                        Twine(NamePrefix) + ".pop.a");
    Value *PopCleared = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, Cleared,
                                              Twine(NamePrefix) + ".pop.clear");
    Value *Delta =
        B.CreateSub(PopA, PopCleared, Twine(NamePrefix) + ".pop.delta");
    return B.CreateAdd(Cleared, Delta, Twine(NamePrefix) + ".clear.mix");
  }
  case 2: {
    Value *X = A;
    X = B.CreateOr(X, B.CreateLShr(X, ci32(Ctx, 1),
                                   Twine(NamePrefix) + ".smear1.shr"),
                   Twine(NamePrefix) + ".smear1");
    X = B.CreateOr(X, B.CreateLShr(X, ci32(Ctx, 2),
                                   Twine(NamePrefix) + ".smear2.shr"),
                   Twine(NamePrefix) + ".smear2");
    X = B.CreateOr(X, B.CreateLShr(X, ci32(Ctx, 4),
                                   Twine(NamePrefix) + ".smear4.shr"),
                   Twine(NamePrefix) + ".smear4");
    X = B.CreateOr(X, B.CreateLShr(X, ci32(Ctx, 8),
                                   Twine(NamePrefix) + ".smear8.shr"),
                   Twine(NamePrefix) + ".smear8");
    X = B.CreateOr(X, B.CreateLShr(X, ci32(Ctx, 16),
                                   Twine(NamePrefix) + ".smear16.shr"),
                   Twine(NamePrefix) + ".smear16");
    Value *Pop =
        callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, X,
                              Twine(NamePrefix) + ".smear.pop");
    return B.CreateSub(X, Pop, Twine(NamePrefix) + ".smear.mix");
  }
  case 3: {
    Value *Byte = B.CreateAnd(A, ci32(Ctx, 0xff), Twine(NamePrefix) + ".byte");
    Value *Pop = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, Byte,
                                       Twine(NamePrefix) + ".byte.pop");
    Value *Parity =
        B.CreateAnd(Pop, ci32(Ctx, 1), Twine(NamePrefix) + ".parity");
    Value *Odd =
        B.CreateICmpNE(Parity, ci32(Ctx, 0), Twine(NamePrefix) + ".odd");
    return B.CreateSelect(Odd, Bv, A, Twine(NamePrefix) + ".parity.select");
  }
  case 4: {
    Value *PopA = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, A,
                                        Twine(NamePrefix) + ".pop.a");
    Value *PopB = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, Bv,
                                        Twine(NamePrefix) + ".pop.b");
    Value *Cmp =
        B.CreateICmpUGT(PopA, PopB, Twine(NamePrefix) + ".pop.cmp");
    return B.CreateSelect(Cmp, A, Bv, Twine(NamePrefix) + ".pop.select");
  }
  case 5: {
    Value *Leading = callI32UnaryIntrinsic(B, M, Intrinsic::ctlz, A,
                                           Twine(NamePrefix) + ".ctlz");
    Value *Trailing = callI32UnaryIntrinsic(B, M, Intrinsic::cttz, Bv,
                                            Twine(NamePrefix) + ".cttz");
    Value *Shift =
        B.CreateAnd(Trailing, ci32(Ctx, 31), Twine(NamePrefix) + ".shift");
    Value *Bit =
        B.CreateShl(ci32(Ctx, 1), Shift, Twine(NamePrefix) + ".bit");
    return B.CreateXor(Bit, Leading, Twine(NamePrefix) + ".count.bit.xor");
  }
  case 6: {
    Value *Rev = callI32UnaryIntrinsic(B, M, Intrinsic::bitreverse, A,
                                       Twine(NamePrefix) + ".reverse");
    Value *Hi = B.CreateLShr(Rev, ci32(Ctx, 16), Twine(NamePrefix) + ".hi");
    Value *Fold =
        B.CreateXor(Rev, Hi, Twine(NamePrefix) + ".reverse.fold");
    Value *Pop = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, Fold,
                                       Twine(NamePrefix) + ".reverse.pop");
    return B.CreateAdd(Fold, Pop, Twine(NamePrefix) + ".reverse.mix");
  }
  default: {
    Value *Diff = B.CreateXor(A, Bv, Twine(NamePrefix) + ".diff");
    Value *Pop = callI32UnaryIntrinsic(B, M, Intrinsic::ctpop, Diff,
                                       Twine(NamePrefix) + ".diff.pop");
    Value *Shift = B.CreateAnd(Pop, ci32(Ctx, 31),
                               Twine(NamePrefix) + ".diff.shift");
    Value *Bit =
        B.CreateShl(ci32(Ctx, 1), Shift, Twine(NamePrefix) + ".diff.bit");
    Value *HasBit = B.CreateICmpNE(
        B.CreateAnd(A, Bit, Twine(NamePrefix) + ".test"), ci32(Ctx, 0),
        Twine(NamePrefix) + ".hasbit");
    Value *Set = B.CreateOr(A, Bit, Twine(NamePrefix) + ".setbit");
    Value *Toggled = B.CreateXor(A, Bit, Twine(NamePrefix) + ".togglebit");
    return B.CreateSelect(HasBit, Toggled, Set,
                          Twine(NamePrefix) + ".bit.select");
  }
  }
}

Value *emitRandomUnsignedSelectIdiom(IRBuilder<NoFolder> &B, Value *A,
                                     Value *Bv, std::minstd_rand &Gen,
                                     StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  switch (Gen() % 6) {
  case 0: {
    Value *Sum = B.CreateAdd(A, Bv, Twine(NamePrefix) + ".uadd");
    Value *Overflow =
        B.CreateICmpULT(Sum, A, Twine(NamePrefix) + ".uadd.ov");
    return B.CreateSelect(Overflow, ci32(Ctx, 0xffffffffu), Sum,
                          Twine(NamePrefix) + ".uadd.sat");
  }
  case 1: {
    Value *Diff = B.CreateSub(A, Bv, Twine(NamePrefix) + ".usub");
    Value *Underflow =
        B.CreateICmpULT(A, Bv, Twine(NamePrefix) + ".usub.ov");
    return B.CreateSelect(Underflow, ci32(Ctx, 0), Diff,
                          Twine(NamePrefix) + ".usub.sat");
  }
  case 2: {
    Value *Cmp = B.CreateICmpULT(A, Bv, Twine(NamePrefix) + ".umin.cmp");
    return B.CreateSelect(Cmp, A, Bv, Twine(NamePrefix) + ".umin");
  }
  case 3: {
    Value *Cmp = B.CreateICmpUGT(A, Bv, Twine(NamePrefix) + ".umax.cmp");
    return B.CreateSelect(Cmp, A, Bv, Twine(NamePrefix) + ".umax");
  }
  case 4: {
    Value *Cmp = B.CreateICmpSLT(A, Bv, Twine(NamePrefix) + ".smin.cmp");
    return B.CreateSelect(Cmp, A, Bv, Twine(NamePrefix) + ".smin");
  }
  default: {
    Value *Cmp = B.CreateICmpSGT(A, Bv, Twine(NamePrefix) + ".smax.cmp");
    return B.CreateSelect(Cmp, A, Bv, Twine(NamePrefix) + ".smax");
  }
  }
}

struct SignedOverflowInfo {
  Value *Wrapped;
  Value *Overflow;
  Value *NegativeOverflow;
};

SignedOverflowInfo buildSignedAddOverflowInfo(IRBuilder<NoFolder> &B, Value *A,
                                              Value *Bv,
                                              const Twine &NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *Sum = B.CreateAdd(A, Bv, Twine(NamePrefix) + ".sadd");
  Value *SameSignBits =
      B.CreateXor(B.CreateXor(A, Bv, Twine(NamePrefix) + ".sadd.abxor"),
                  ci32(Ctx, 0xffffffffu), Twine(NamePrefix) + ".sadd.same");
  Value *SignFlip = B.CreateXor(A, Sum, Twine(NamePrefix) + ".sadd.flip");
  Value *OverflowBits =
      B.CreateAnd(SameSignBits, SignFlip, Twine(NamePrefix) + ".sadd.ovbits");
  Value *Overflow =
      B.CreateICmpSLT(OverflowBits, ci32(Ctx, 0), Twine(NamePrefix) + ".sadd.ov");
  Value *NegativeOverflow =
      B.CreateICmpSLT(A, ci32(Ctx, 0), Twine(NamePrefix) + ".sadd.neg");
  return {Sum, Overflow, NegativeOverflow};
}

SignedOverflowInfo buildSignedSubOverflowInfo(IRBuilder<NoFolder> &B, Value *A,
                                              Value *Bv,
                                              const Twine &NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *Diff = B.CreateSub(A, Bv, Twine(NamePrefix) + ".ssub");
  Value *DifferentSignBits =
      B.CreateXor(A, Bv, Twine(NamePrefix) + ".ssub.abxor");
  Value *SignFlip = B.CreateXor(A, Diff, Twine(NamePrefix) + ".ssub.flip");
  Value *OverflowBits = B.CreateAnd(DifferentSignBits, SignFlip,
                                    Twine(NamePrefix) + ".ssub.ovbits");
  Value *Overflow =
      B.CreateICmpSLT(OverflowBits, ci32(Ctx, 0), Twine(NamePrefix) + ".ssub.ov");
  Value *NegativeOverflow =
      B.CreateICmpSLT(A, ci32(Ctx, 0), Twine(NamePrefix) + ".ssub.neg");
  return {Diff, Overflow, NegativeOverflow};
}

Value *signedSaturatingSelect(IRBuilder<NoFolder> &B,
                              const SignedOverflowInfo &Info,
                              const Twine &NamePrefix) {
  LLVMContext &Ctx = Info.Wrapped->getContext();
  Value *SatValue =
      B.CreateSelect(Info.NegativeOverflow, ci32(Ctx, 0x80000000u),
                     ci32(Ctx, 0x7fffffffu),
                     Twine(NamePrefix) + ".sat.value");
  return B.CreateSelect(Info.Overflow, SatValue, Info.Wrapped,
                        Twine(NamePrefix) + ".sat");
}

Value *emitRandomSignedOverflowSelectIdiom(IRBuilder<NoFolder> &B, Value *A,
                                           Value *Bv, std::minstd_rand &Gen,
                                           StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  switch (Gen() % 6) {
  case 0:
    return signedSaturatingSelect(
        B, buildSignedAddOverflowInfo(B, A, Bv, Twine(NamePrefix) + ".add"),
        Twine(NamePrefix) + ".sadd");
  case 1:
    return signedSaturatingSelect(
        B, buildSignedSubOverflowInfo(B, A, Bv, Twine(NamePrefix) + ".sub"),
        Twine(NamePrefix) + ".ssub");
  case 2: {
    SignedOverflowInfo Info =
        buildSignedAddOverflowInfo(B, A, Bv, Twine(NamePrefix) + ".add.flag");
    Value *OverflowI32 =
        B.CreateZExt(Info.Overflow, I32, Twine(NamePrefix) + ".sadd.ov.i32");
    return B.CreateXor(Info.Wrapped, OverflowI32,
                       Twine(NamePrefix) + ".sadd.xor");
  }
  case 3: {
    SignedOverflowInfo Info =
        buildSignedSubOverflowInfo(B, A, Bv, Twine(NamePrefix) + ".sub.flag");
    Value *OverflowI32 =
        B.CreateZExt(Info.Overflow, I32, Twine(NamePrefix) + ".ssub.ov.i32");
    return B.CreateAdd(Info.Wrapped, OverflowI32,
                       Twine(NamePrefix) + ".ssub.add");
  }
  case 4: {
    SignedOverflowInfo Info =
        buildSignedAddOverflowInfo(B, A, Bv, Twine(NamePrefix) + ".add.sel");
    return B.CreateSelect(Info.Overflow, Bv, Info.Wrapped,
                          Twine(NamePrefix) + ".sadd.select");
  }
  default: {
    SignedOverflowInfo Info =
        buildSignedSubOverflowInfo(B, A, Bv, Twine(NamePrefix) + ".sub.sel");
    return B.CreateSelect(Info.Overflow, A, Info.Wrapped,
                          Twine(NamePrefix) + ".ssub.select");
  }
  }
}

Value *emitRandomManualFunnelShiftIdiom(IRBuilder<NoFolder> &B, Value *A,
                                        Value *Bv, std::minstd_rand &Gen,
                                        StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *ShiftSeed = nullptr;
  switch (Gen() % 4) {
  case 0:
    ShiftSeed = A;
    break;
  case 1:
    ShiftSeed = Bv;
    break;
  case 2:
    ShiftSeed = ci32(Ctx, Gen() & 31u);
    break;
  default:
    ShiftSeed = interestingI32(Ctx, Gen);
    break;
  }
  Value *Shift = B.CreateAnd(ShiftSeed, ci32(Ctx, 31),
                             Twine(NamePrefix) + ".shift");
  Value *InvShift =
      B.CreateAnd(B.CreateSub(ci32(Ctx, 32), Shift,
                              Twine(NamePrefix) + ".inv.raw"),
                  ci32(Ctx, 31), Twine(NamePrefix) + ".inv");
  Value *Zero =
      B.CreateICmpEQ(Shift, ci32(Ctx, 0), Twine(NamePrefix) + ".zero");

  if ((Gen() % 2) == 0) {
    Value *Lo = B.CreateShl(A, Shift, Twine(NamePrefix) + ".left");
    Value *Hi = B.CreateLShr(Bv, InvShift, Twine(NamePrefix) + ".right");
    Value *Merged = B.CreateOr(Lo, Hi, Twine(NamePrefix) + ".fshl.raw");
    return B.CreateSelect(Zero, A, Merged, Twine(NamePrefix) + ".fshl");
  }

  Value *Hi = B.CreateShl(A, InvShift, Twine(NamePrefix) + ".left");
  Value *Lo = B.CreateLShr(Bv, Shift, Twine(NamePrefix) + ".right");
  Value *Merged = B.CreateOr(Hi, Lo, Twine(NamePrefix) + ".fshr.raw");
  return B.CreateSelect(Zero, Bv, Merged, Twine(NamePrefix) + ".fshr");
}

FCmpInst::Predicate randomFCmpPredicate(std::minstd_rand &Gen) {
  static constexpr std::array<FCmpInst::Predicate, 6> Predicates = {
      FCmpInst::FCMP_OEQ, FCmpInst::FCMP_ONE, FCmpInst::FCMP_OGT,
      FCmpInst::FCMP_OGE, FCmpInst::FCMP_OLT, FCmpInst::FCMP_OLE};
  return Predicates[Gen() % Predicates.size()];
}

Value *emitRandomFiniteFPInstruction(IRBuilder<NoFolder> &B, Value *A,
                                     Value *Bv, std::minstd_rand &Gen,
                                     StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  Type *F64 = Type::getDoubleTy(Ctx);
  Value *AMasked = B.CreateAnd(A, ci32(Ctx, 1023),
                               Twine(NamePrefix) + ".mask.a");
  Value *BMasked = B.CreateAnd(Bv, ci32(Ctx, 1023),
                               Twine(NamePrefix) + ".mask.b");
  Value *FA = B.CreateUIToFP(AMasked, F32, Twine(NamePrefix) + ".uitofp.a");
  Value *FB = B.CreateUIToFP(BMasked, F32, Twine(NamePrefix) + ".uitofp.b");

  Value *Result = nullptr;
  switch (Gen() % 8) {
  case 0:
    Result = B.CreateFAdd(FA, FB, Twine(NamePrefix) + ".fadd");
    break;
  case 1:
    Result = B.CreateFMul(FA, FB, Twine(NamePrefix) + ".fmul");
    break;
  case 2: {
    Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), FA, FB,
                              Twine(NamePrefix) + ".fcmp");
    Result = B.CreateSelect(Cmp, FA, FB, Twine(NamePrefix) + ".select");
    break;
  }
  case 3: {
    Value *Product = B.CreateFMul(FA, FB, Twine(NamePrefix) + ".fmul");
    Result = B.CreateFAdd(Product, FA, Twine(NamePrefix) + ".fmaish");
    break;
  }
  case 4: {
    Value *FA64 = B.CreateFPExt(FA, F64, Twine(NamePrefix) + ".fpext.a");
    Value *FB64 = B.CreateFPExt(FB, F64, Twine(NamePrefix) + ".fpext.b");
    Value *F64Result =
        B.CreateFAdd(FA64, FB64, Twine(NamePrefix) + ".f64.add");
    Result = B.CreateFPTrunc(F64Result, F32, Twine(NamePrefix) + ".fptrunc");
    break;
  }
  case 5: {
    Value *Small = B.CreateAnd(Bv, ci32(Ctx, 15),
                               Twine(NamePrefix) + ".small");
    Value *FSmall =
        B.CreateUIToFP(Small, F32, Twine(NamePrefix) + ".uitofp.small");
    Result = B.CreateFMul(FA, FSmall, Twine(NamePrefix) + ".smallmul");
    break;
  }
  case 6: {
    Value *Den = B.CreateOr(B.CreateAnd(Bv, ci32(Ctx, 31),
                                        Twine(NamePrefix) + ".den.mask"),
                            ci32(Ctx, 1), Twine(NamePrefix) + ".den.nz");
    Value *FDen = B.CreateUIToFP(Den, F32, Twine(NamePrefix) + ".uitofp.den");
    Result = B.CreateFDiv(FA, FDen, Twine(NamePrefix) + ".fdiv");
    break;
  }
  default: {
    Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), FA, FB,
                              Twine(NamePrefix) + ".fcmp");
    Value *Selected = B.CreateSelect(Cmp, AMasked, BMasked,
                                     Twine(NamePrefix) + ".i32.select");
    Result = B.CreateUIToFP(Selected, F32, Twine(NamePrefix) + ".uitofp.sel");
    break;
  }
  }

  Value *IntResult =
      B.CreateFPToUI(Result, I32, Twine(NamePrefix) + ".fptoui");
  switch (Gen() % 4) {
  case 0:
    return IntResult;
  case 1:
    return B.CreateXor(IntResult, A, Twine(NamePrefix) + ".xor");
  case 2:
    return B.CreateAdd(IntResult, Bv, Twine(NamePrefix) + ".add");
  default:
    return B.CreateOr(IntResult, A, Twine(NamePrefix) + ".or");
  }
}

Value *emitRandomFiniteHalfFPInstruction(IRBuilder<NoFolder> &B, Value *A,
                                         Value *Bv, std::minstd_rand &Gen,
                                         StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I4 = Type::getIntNTy(Ctx, 4);
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F16 = Type::getHalfTy(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  bool UseSigned = (Gen() % 3) == 0;

  Value *IntResult = nullptr;
  if (!UseSigned) {
    Value *AMasked = B.CreateAnd(A, ci32(Ctx, 127),
                                 Twine(NamePrefix) + ".mask.a");
    Value *BMasked = B.CreateAnd(Bv, ci32(Ctx, 127),
                                 Twine(NamePrefix) + ".mask.b");
    Value *FA = B.CreateUIToFP(AMasked, F16, Twine(NamePrefix) + ".uitofp.a");
    Value *FB = B.CreateUIToFP(BMasked, F16, Twine(NamePrefix) + ".uitofp.b");
    Value *Small = B.CreateAnd(Bv, ci32(Ctx, 15),
                               Twine(NamePrefix) + ".small");
    Value *FSmall =
        B.CreateUIToFP(Small, F16, Twine(NamePrefix) + ".uitofp.small");

    Value *Result = nullptr;
    switch (Gen() % 8) {
    case 0:
      Result = B.CreateFAdd(FA, FB, Twine(NamePrefix) + ".fadd");
      break;
    case 1:
      Result = B.CreateFMul(FA, FB, Twine(NamePrefix) + ".fmul");
      break;
    case 2: {
      Value *Product = B.CreateFMul(FA, FSmall, Twine(NamePrefix) + ".fmul");
      Result = B.CreateFAdd(Product, FB, Twine(NamePrefix) + ".fmaish");
      break;
    }
    case 3: {
      Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), FA, FB,
                                Twine(NamePrefix) + ".fcmp");
      Result = B.CreateSelect(Cmp, FA, FB, Twine(NamePrefix) + ".select");
      break;
    }
    case 4: {
      Value *FA32 = B.CreateFPExt(FA, F32, Twine(NamePrefix) + ".fpext.a");
      Value *FB32 = B.CreateFPExt(FB, F32, Twine(NamePrefix) + ".fpext.b");
      Value *F32Result =
          B.CreateFAdd(FA32, FB32, Twine(NamePrefix) + ".f32.add");
      Result = B.CreateFPTrunc(F32Result, F16, Twine(NamePrefix) + ".fptrunc");
      break;
    }
    case 5: {
      Value *Cmp = B.CreateFCmpOGE(FA, FB, Twine(NamePrefix) + ".oge");
      Value *Hi = B.CreateSelect(Cmp, FA, FB, Twine(NamePrefix) + ".hi");
      Value *Lo = B.CreateSelect(Cmp, FB, FA, Twine(NamePrefix) + ".lo");
      Result = B.CreateFSub(Hi, Lo, Twine(NamePrefix) + ".fsub");
      break;
    }
    case 6: {
      Value *Den = B.CreateOr(B.CreateAnd(Bv, ci32(Ctx, 15),
                                          Twine(NamePrefix) + ".den.mask"),
                              ci32(Ctx, 1), Twine(NamePrefix) + ".den.nz");
      Value *FDen =
          B.CreateUIToFP(Den, F16, Twine(NamePrefix) + ".uitofp.den");
      Result = B.CreateFDiv(FA, FDen, Twine(NamePrefix) + ".fdiv");
      break;
    }
    default: {
      Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), FA, FB,
                                Twine(NamePrefix) + ".fcmp");
      Value *Selected = B.CreateSelect(Cmp, AMasked, BMasked,
                                       Twine(NamePrefix) + ".i32.select");
      Result = B.CreateUIToFP(Selected, F16,
                              Twine(NamePrefix) + ".uitofp.sel");
      break;
    }
    }
    IntResult = B.CreateFPToUI(Result, I32, Twine(NamePrefix) + ".fptoui");
  } else {
    Value *ASmall =
        B.CreateSExt(B.CreateTrunc(A, I8, Twine(NamePrefix) + ".trunc.a"),
                     I32, Twine(NamePrefix) + ".sext.a");
    Value *BSmall =
        B.CreateSExt(B.CreateTrunc(Bv, I8, Twine(NamePrefix) + ".trunc.b"),
                     I32, Twine(NamePrefix) + ".sext.b");
    Value *FA = B.CreateSIToFP(ASmall, F16, Twine(NamePrefix) + ".sitofp.a");
    Value *FB = B.CreateSIToFP(BSmall, F16, Twine(NamePrefix) + ".sitofp.b");
    Value *Tiny =
        B.CreateSExt(B.CreateTrunc(Bv, I4, Twine(NamePrefix) + ".tiny.trunc"),
                     I32, Twine(NamePrefix) + ".tiny.sext");
    Value *FTiny =
        B.CreateSIToFP(Tiny, F16, Twine(NamePrefix) + ".sitofp.tiny");

    Value *Result = nullptr;
    switch (Gen() % 7) {
    case 0:
      Result = B.CreateFAdd(FA, FB, Twine(NamePrefix) + ".fadd");
      break;
    case 1:
      Result = B.CreateFSub(FA, FB, Twine(NamePrefix) + ".fsub");
      break;
    case 2:
      Result = B.CreateFMul(FA, FTiny, Twine(NamePrefix) + ".tinymul");
      break;
    case 3: {
      Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), FA, FB,
                                Twine(NamePrefix) + ".fcmp");
      Result = B.CreateSelect(Cmp, FA, FB, Twine(NamePrefix) + ".select");
      break;
    }
    case 4: {
      Value *FA32 = B.CreateFPExt(FA, F32, Twine(NamePrefix) + ".fpext.a");
      Value *FB32 = B.CreateFPExt(FB, F32, Twine(NamePrefix) + ".fpext.b");
      Value *F32Result =
          B.CreateFSub(FA32, FB32, Twine(NamePrefix) + ".f32.sub");
      Result = B.CreateFPTrunc(F32Result, F16, Twine(NamePrefix) + ".fptrunc");
      break;
    }
    case 5: {
      Value *Den = B.CreateOr(B.CreateAnd(Bv, ci32(Ctx, 15),
                                          Twine(NamePrefix) + ".den.mask"),
                              ci32(Ctx, 1), Twine(NamePrefix) + ".den.nz");
      Value *FDen =
          B.CreateUIToFP(Den, F16, Twine(NamePrefix) + ".uitofp.den");
      Result = B.CreateFDiv(FA, FDen, Twine(NamePrefix) + ".fdiv");
      break;
    }
    default:
      Result = B.CreateFMul(FB, FTiny, Twine(NamePrefix) + ".tinymul");
      break;
    }
    IntResult = B.CreateFPToSI(Result, I32, Twine(NamePrefix) + ".fptosi");
  }

  switch (Gen() % 4) {
  case 0:
    return IntResult;
  case 1:
    return B.CreateXor(IntResult, A, Twine(NamePrefix) + ".xor");
  case 2:
    return B.CreateAdd(IntResult, Bv, Twine(NamePrefix) + ".add");
  default:
    return B.CreateSub(A, IntResult, Twine(NamePrefix) + ".sub");
  }
}

Value *emitRandomFiniteSignedFPInstruction(IRBuilder<NoFolder> &B, Value *A,
                                           Value *Bv, std::minstd_rand &Gen,
                                           StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  Type *F64 = Type::getDoubleTy(Ctx);
  Value *ASmall =
      B.CreateSExt(B.CreateTrunc(A, I8, Twine(NamePrefix) + ".trunc.a"), I32,
                   Twine(NamePrefix) + ".sext.a");
  Value *BSmall =
      B.CreateSExt(B.CreateTrunc(Bv, I8, Twine(NamePrefix) + ".trunc.b"), I32,
                   Twine(NamePrefix) + ".sext.b");
  Value *FA = B.CreateSIToFP(ASmall, F32, Twine(NamePrefix) + ".sitofp.a");
  Value *FB = B.CreateSIToFP(BSmall, F32, Twine(NamePrefix) + ".sitofp.b");

  Value *Result = nullptr;
  switch (Gen() % 7) {
  case 0:
    Result = B.CreateFAdd(FA, FB, Twine(NamePrefix) + ".fadd");
    break;
  case 1:
    Result = B.CreateFSub(FA, FB, Twine(NamePrefix) + ".fsub");
    break;
  case 2:
    Result = B.CreateFMul(FA, FB, Twine(NamePrefix) + ".fmul");
    break;
  case 3: {
    Value *Cmp = B.CreateFCmp(randomFCmpPredicate(Gen), FA, FB,
                              Twine(NamePrefix) + ".fcmp");
    Result = B.CreateSelect(Cmp, FA, FB, Twine(NamePrefix) + ".select");
    break;
  }
  case 4: {
    Value *FA64 = B.CreateFPExt(FA, F64, Twine(NamePrefix) + ".fpext.a");
    Value *FB64 = B.CreateFPExt(FB, F64, Twine(NamePrefix) + ".fpext.b");
    Value *F64Result =
        B.CreateFSub(FA64, FB64, Twine(NamePrefix) + ".f64.sub");
    Result = B.CreateFPTrunc(F64Result, F32, Twine(NamePrefix) + ".fptrunc");
    break;
  }
  case 5: {
    Value *Den = B.CreateOr(B.CreateAnd(Bv, ci32(Ctx, 31),
                                        Twine(NamePrefix) + ".den.mask"),
                            ci32(Ctx, 1), Twine(NamePrefix) + ".den.nz");
    Value *FDen = B.CreateUIToFP(Den, F32, Twine(NamePrefix) + ".uitofp.den");
    Result = B.CreateFDiv(FA, FDen, Twine(NamePrefix) + ".fdiv");
    break;
  }
  default: {
    Value *Tiny =
        B.CreateSExt(B.CreateTrunc(Bv, Type::getIntNTy(Ctx, 4),
                                   Twine(NamePrefix) + ".tiny.trunc"),
                     I32, Twine(NamePrefix) + ".tiny.sext");
    Value *FTiny = B.CreateSIToFP(Tiny, F32, Twine(NamePrefix) + ".sitofp.tiny");
    Result = B.CreateFMul(FA, FTiny, Twine(NamePrefix) + ".tinymul");
    break;
  }
  }

  Value *IntResult =
      B.CreateFPToSI(Result, I32, Twine(NamePrefix) + ".fptosi");
  switch (Gen() % 4) {
  case 0:
    return IntResult;
  case 1:
    return B.CreateXor(IntResult, A, Twine(NamePrefix) + ".xor");
  case 2:
    return B.CreateAdd(IntResult, Bv, Twine(NamePrefix) + ".add");
  default:
    return B.CreateSub(A, IntResult, Twine(NamePrefix) + ".sub");
  }
}

Intrinsic::ID randomOverflowIntrinsic(std::minstd_rand &Gen) {
  static constexpr std::array<Intrinsic::ID, 6> IDs = {
      Intrinsic::uadd_with_overflow, Intrinsic::usub_with_overflow,
      Intrinsic::umul_with_overflow, Intrinsic::sadd_with_overflow,
      Intrinsic::ssub_with_overflow, Intrinsic::smul_with_overflow};
  return IDs[Gen() % IDs.size()];
}

Value *emitRandomOverflowInstruction(IRBuilder<NoFolder> &B, Module &M,
                                     Value *A, Value *Bv,
                                     std::minstd_rand &Gen,
                                     StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  FunctionCallee Fn =
      Intrinsic::getOrInsertDeclaration(&M, randomOverflowIntrinsic(Gen), {I32});
  Value *Pair = B.CreateCall(Fn, {A, Bv}, Twine(NamePrefix) + ".call");
  Value *Result = B.CreateExtractValue(Pair, {0}, Twine(NamePrefix) + ".value");
  Value *Overflow =
      B.CreateExtractValue(Pair, {1}, Twine(NamePrefix) + ".overflow");
  Value *OverflowI32 =
      B.CreateZExt(Overflow, I32, Twine(NamePrefix) + ".overflow.i32");
  switch (Gen() % 5) {
  case 0:
    return Result;
  case 1:
    return B.CreateXor(Result, OverflowI32, Twine(NamePrefix) + ".xor");
  case 2:
    return B.CreateAdd(Result, OverflowI32, Twine(NamePrefix) + ".add");
  case 3:
    return B.CreateSub(Result, OverflowI32, Twine(NamePrefix) + ".sub");
  default:
    return B.CreateSelect(Overflow, Bv, Result, Twine(NamePrefix) + ".select");
  }
}

Value *packI32ToV2I16(IRBuilder<NoFolder> &B, Value *V, const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *V2I16 = FixedVectorType::get(I16, 2);
  Value *Lo = B.CreateTrunc(V, I16, Twine(Name) + ".lo");
  Value *Hi32 = B.CreateLShr(V, ConstantInt::get(I32, 16),
                             Twine(Name) + ".hi32");
  Value *Hi = B.CreateTrunc(Hi32, I16, Twine(Name) + ".hi");
  Value *Packed = Constant::getNullValue(V2I16);
  Packed = B.CreateInsertElement(Packed, Lo, ConstantInt::get(I32, 0),
                                 Twine(Name) + ".ins0");
  return B.CreateInsertElement(Packed, Hi, ConstantInt::get(I32, 1),
                               Twine(Name) + ".ins1");
}

Value *packI32PairToI64(IRBuilder<NoFolder> &B, Value *Lo, Value *Hi,
                        const Twine &Name) {
  LLVMContext &Ctx = Lo->getContext();
  Type *I64 = Type::getInt64Ty(Ctx);
  Value *Lo64 = B.CreateZExt(Lo, I64, Twine(Name) + ".lo64");
  Value *Hi64 = B.CreateZExt(Hi, I64, Twine(Name) + ".hi64");
  Hi64 = B.CreateShl(Hi64, ConstantInt::get(I64, 32),
                     Twine(Name) + ".hi.shift");
  return B.CreateOr(Lo64, Hi64, Twine(Name) + ".pack");
}

Value *foldI64ToI32(IRBuilder<NoFolder> &B, Value *V, const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Value *Lo = B.CreateTrunc(V, I32, Twine(Name) + ".lo");
  Value *Hi = B.CreateLShr(V, ConstantInt::get(I64, 32),
                           Twine(Name) + ".hi64");
  Hi = B.CreateTrunc(Hi, I32, Twine(Name) + ".hi");
  return B.CreateXor(Lo, Hi, Twine(Name) + ".xor");
}

Value *reduceV4I32ToI32(IRBuilder<NoFolder> &B, Value *V,
                        std::minstd_rand &Gen, const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Value *A = B.CreateExtractElement(V, ci32(Ctx, Gen() & 3u),
                                    Twine(Name) + ".a");
  Value *Bv = B.CreateExtractElement(V, ci32(Ctx, Gen() & 3u),
                                     Twine(Name) + ".b");
  switch (Gen() % 4) {
  case 0:
    return B.CreateXor(A, Bv, Twine(Name) + ".xor");
  case 1:
    return B.CreateAdd(A, Bv, Twine(Name) + ".add");
  case 2:
    return B.CreateOr(A, Bv, Twine(Name) + ".or");
  default:
    return B.CreateSub(A, Bv, Twine(Name) + ".sub");
  }
}

Value *boundedUIToF32(IRBuilder<NoFolder> &B, Value *V, uint32_t Mask,
                      const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Value *Masked = B.CreateAnd(V, ci32(Ctx, Mask), Twine(Name) + ".mask");
  return B.CreateUIToFP(Masked, Type::getFloatTy(Ctx),
                        Twine(Name) + ".uitofp");
}

Value *positiveUIToF32(IRBuilder<NoFolder> &B, Value *V, uint32_t Mask,
                       const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Value *Masked = B.CreateAnd(V, ci32(Ctx, Mask), Twine(Name) + ".mask");
  Value *Positive = B.CreateOr(Masked, ci32(Ctx, 1), Twine(Name) + ".nz");
  return B.CreateUIToFP(Positive, Type::getFloatTy(Ctx),
                        Twine(Name) + ".uitofp");
}

Value *mixI32IntrinsicResult(IRBuilder<NoFolder> &B, Value *Result, Value *A,
                             Value *Bv, std::minstd_rand &Gen,
                             const Twine &Name) {
  switch (Gen() % 4) {
  case 0:
    return Result;
  case 1:
    return B.CreateXor(Result, A, Twine(Name) + ".xor");
  case 2:
    return B.CreateAdd(Result, Bv, Twine(Name) + ".add");
  default:
    return B.CreateSub(A, Result, Twine(Name) + ".sub");
  }
}

Value *fp32ToBoundedI32(IRBuilder<NoFolder> &B, Value *F, Value *A, Value *Bv,
                        std::minstd_rand &Gen, const Twine &Name) {
  Type *I32 = Type::getInt32Ty(F->getContext());
  Value *Int = B.CreateFPToUI(F, I32, Twine(Name) + ".fptoui");
  return mixI32IntrinsicResult(B, Int, A, Bv, Gen, Name);
}

Value *bitcastPackedV2ToI32(IRBuilder<NoFolder> &B, Value *V,
                            const Twine &Name) {
  return B.CreateBitCast(V, Type::getInt32Ty(V->getContext()),
                         Twine(Name) + ".bits");
}

Value *emitRandomAMDGPUFPIntrinsicInstruction(IRBuilder<NoFolder> &B, Module &M,
                                              Value *A, Value *Bv,
                                              std::minstd_rand &Gen,
                                              StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *F32 = Type::getFloatTy(Ctx);
  Value *FA = boundedUIToF32(B, A, 31, Twine(NamePrefix) + ".fp.a");
  Value *FB = boundedUIToF32(B, Bv, 31, Twine(NamePrefix) + ".fp.b");
  Value *FC = boundedUIToF32(B, interestingI32(Ctx, Gen), 31,
                             Twine(NamePrefix) + ".fp.c");
  bool AllowFMALegacy =
      envFlag("FUZZX_ALLOW_C002_FMA_LEGACY_ISEL_ICE", false);

  unsigned Choice = Gen() % (AllowFMALegacy ? 10 : 9);
  if (!AllowFMALegacy)
    ++Choice;

  switch (Choice) {
  case 0: {
    Value *Mul = boundedUIToF32(B, Bv, 15, Twine(NamePrefix) + ".fma.mul");
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_fma_legacy),
        {FA, Mul, FC}, Twine(NamePrefix) + ".fma.legacy");
    return fp32ToBoundedI32(B, R, A, Bv, Gen, Twine(NamePrefix) + ".fma");
  }
  case 1: {
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_fmed3, {F32}),
        {FA, FB, FC}, Twine(NamePrefix) + ".fmed3");
    return fp32ToBoundedI32(B, R, A, Bv, Gen, Twine(NamePrefix) + ".fmed3");
  }
  case 2: {
    static constexpr std::array<uint32_t, 12> Masks = {
        1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 96, 1023};
    Value *Mask = ci32(Ctx, Masks[Gen() % Masks.size()]);
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_class, {F32}),
        {FA, Mask}, Twine(NamePrefix) + ".class");
    Value *Z = B.CreateZExt(R, I32, Twine(NamePrefix) + ".class.zext");
    return mixI32IntrinsicResult(B, Z, A, Bv, Gen,
                                 Twine(NamePrefix) + ".class");
  }
  case 3: {
    Value *FPos = positiveUIToF32(B, A, 1023, Twine(NamePrefix) + ".frexp");
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_frexp_exp,
                                          {I32, F32}),
        {FPos}, Twine(NamePrefix) + ".frexp.exp");
    return mixI32IntrinsicResult(B, R, A, Bv, Gen,
                                 Twine(NamePrefix) + ".frexp.exp");
  }
  case 4: {
    Value *FPos = positiveUIToF32(B, A, 1023, Twine(NamePrefix) + ".frexp");
    Value *Mant = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_frexp_mant,
                                          {F32}),
        {FPos}, Twine(NamePrefix) + ".frexp.mant");
    Value *Scaled = B.CreateFMul(
        Mant, ConstantFP::get(F32, 1024.0), Twine(NamePrefix) + ".mant.scale");
    return fp32ToBoundedI32(B, Scaled, A, Bv, Gen,
                            Twine(NamePrefix) + ".frexp.mant");
  }
  case 5: {
    Value *Base = boundedUIToF32(B, A, 31, Twine(NamePrefix) + ".fract.base");
    Value *FracInput = B.CreateFAdd(Base, ConstantFP::get(F32, 0.5),
                                    Twine(NamePrefix) + ".fract.input");
    Value *Frac = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_fract, {F32}),
        {FracInput}, Twine(NamePrefix) + ".fract");
    Value *Scaled = B.CreateFMul(
        Frac, ConstantFP::get(F32, 1024.0), Twine(NamePrefix) + ".fract.scale");
    return fp32ToBoundedI32(B, Scaled, A, Bv, Gen,
                            Twine(NamePrefix) + ".fract");
  }
  case 6: {
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_cvt_pkrtz),
        {FA, FB}, Twine(NamePrefix) + ".cvt.pkrtz");
    return mixI32IntrinsicResult(
        B, bitcastPackedV2ToI32(B, R, Twine(NamePrefix) + ".cvt.pkrtz"), A, Bv,
        Gen, Twine(NamePrefix) + ".cvt.pkrtz");
  }
  case 7: {
    Value *NormA = boundedUIToF32(B, A, 1, Twine(NamePrefix) + ".norm.a");
    Value *NormB = boundedUIToF32(B, Bv, 1, Twine(NamePrefix) + ".norm.b");
    Intrinsic::ID ID = (Gen() % 2) == 0 ? Intrinsic::amdgcn_cvt_pknorm_i16
                                        : Intrinsic::amdgcn_cvt_pknorm_u16;
    Value *R = B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID),
                            {NormA, NormB},
                            Twine(NamePrefix) + ".cvt.pknorm");
    return mixI32IntrinsicResult(
        B, bitcastPackedV2ToI32(B, R, Twine(NamePrefix) + ".cvt.pknorm"), A,
        Bv, Gen, Twine(NamePrefix) + ".cvt.pknorm");
  }
  case 8: {
    Intrinsic::ID ID = (Gen() % 2) == 0 ? Intrinsic::amdgcn_cvt_pk_i16
                                        : Intrinsic::amdgcn_cvt_pk_u16;
    Value *R = B.CreateCall(Intrinsic::getOrInsertDeclaration(&M, ID), {A, Bv},
                            Twine(NamePrefix) + ".cvt.pk.i16");
    return mixI32IntrinsicResult(
        B, bitcastPackedV2ToI32(B, R, Twine(NamePrefix) + ".cvt.pk.i16"), A,
        Bv, Gen, Twine(NamePrefix) + ".cvt.pk.i16");
  }
  default: {
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_cvt_pk_u8_f32),
        {FA, A, ci32(Ctx, Gen() & 3u)}, Twine(NamePrefix) + ".cvt.pk.u8");
    return mixI32IntrinsicResult(B, R, A, Bv, Gen,
                                 Twine(NamePrefix) + ".cvt.pk.u8");
  }
  }
}

Intrinsic::ID randomWaveReduceIntrinsic(std::minstd_rand &Gen) {
  SmallVector<Intrinsic::ID, 8> IDs = {
      Intrinsic::amdgcn_wave_reduce_umin, Intrinsic::amdgcn_wave_reduce_min,
      Intrinsic::amdgcn_wave_reduce_umax, Intrinsic::amdgcn_wave_reduce_max,
      Intrinsic::amdgcn_wave_reduce_and,  Intrinsic::amdgcn_wave_reduce_or};
  if (envFlag("FUZZX_ALLOW_M035_WAVE_REDUCE_XOR", false))
    IDs.push_back(Intrinsic::amdgcn_wave_reduce_xor);
  if (envFlag("FUZZX_ALLOW_M036_WAVE_REDUCE_ADD", false))
    IDs.push_back(Intrinsic::amdgcn_wave_reduce_add);
  return IDs[Gen() % IDs.size()];
}

Value *emitRandomAMDGPUWaveInstruction(IRBuilder<NoFolder> &B, Module &M,
                                       Value *A, Value *Bv,
                                       std::minstd_rand &Gen,
                                       StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  switch (Gen() % 2) {
  case 0: {
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_readfirstlane,
                                          {I32}),
        {A}, Twine(NamePrefix) + ".readfirstlane");
    return mixI32IntrinsicResult(B, R, A, Bv, Gen,
                                 Twine(NamePrefix) + ".readfirstlane");
  }
  default: {
    Value *Input = A;
    switch (Gen() % 4) {
    case 0:
      Input = B.CreateXor(A, Bv, Twine(NamePrefix) + ".wave.input.xor");
      break;
    case 1:
      Input = B.CreateAdd(A, Bv, Twine(NamePrefix) + ".wave.input.add");
      break;
    case 2:
      Input = B.CreateAnd(A, Bv, Twine(NamePrefix) + ".wave.input.and");
      break;
    default:
      break;
    }
    Value *Strategy = ci32(Ctx, Gen() % 3);
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, randomWaveReduceIntrinsic(Gen),
                                          {I32}),
        {Input, Strategy}, Twine(NamePrefix) + ".wave.reduce");
    return mixI32IntrinsicResult(B, R, A, Bv, Gen,
                                 Twine(NamePrefix) + ".wave.reduce");
  }
  }
}

Value *emitRandomAMDGPUIntrinsicInstruction(IRBuilder<NoFolder> &B, Module &M,
                                            Value *A, Value *Bv,
                                            std::minstd_rand &Gen,
                                            StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Type *I1 = Type::getInt1Ty(Ctx);
  auto *V4I32 = FixedVectorType::get(I32, 4);
  Value *C = interestingI32(Ctx, Gen);
  Value *Clamp = ConstantInt::getFalse(Ctx);
  bool AllowSUDot = envFlag("FUZZX_ALLOW_C001_SUDOT_ISEL_ICE", false);
  switch (Gen() % (AllowSUDot ? 52 : 50)) {
  case 0: {
    unsigned Offset = Gen() % 32;
    unsigned Width = Gen() % (33 - Offset);
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_ubfe, {I32}),
        {A, ci32(Ctx, Offset), ci32(Ctx, Width)},
        Twine(NamePrefix) + ".ubfe");
  }
  case 1: {
    unsigned Offset = Gen() % 32;
    unsigned Width = Gen() % (33 - Offset);
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sbfe, {I32}),
        {A, ci32(Ctx, Offset), ci32(Ctx, Width)},
        Twine(NamePrefix) + ".sbfe");
  }
  case 2:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_lerp),
        {A, Bv, C}, Twine(NamePrefix) + ".lerp");
  case 3:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sad_u8),
        {A, Bv, C}, Twine(NamePrefix) + ".sad.u8");
  case 4:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_msad_u8),
        {A, Bv, C}, Twine(NamePrefix) + ".msad.u8");
  case 5:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sad_hi_u8),
        {A, Bv, C}, Twine(NamePrefix) + ".sad.hi.u8");
  case 6:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sad_u16),
        {A, Bv, C}, Twine(NamePrefix) + ".sad.u16");
  case 7:
    return foldI64ToI32(
        B,
        B.CreateCall(Intrinsic::getOrInsertDeclaration(
                         &M, Intrinsic::amdgcn_qsad_pk_u16_u8),
                     {packI32PairToI64(B, A, Bv,
                                       Twine(NamePrefix) + ".qsad.a"),
                      C,
                      packI32PairToI64(B, Bv, A,
                                       Twine(NamePrefix) + ".qsad.c")},
                     Twine(NamePrefix) + ".qsad"),
        Twine(NamePrefix) + ".qsad.fold");
  case 8:
    return foldI64ToI32(
        B,
        B.CreateCall(Intrinsic::getOrInsertDeclaration(
                         &M, Intrinsic::amdgcn_mqsad_pk_u16_u8),
                     {packI32PairToI64(B, A, C,
                                       Twine(NamePrefix) + ".mqsad.pk.a"),
                      Bv,
                      packI32PairToI64(B, C, A,
                                       Twine(NamePrefix) + ".mqsad.pk.c")},
                     Twine(NamePrefix) + ".mqsad.pk"),
        Twine(NamePrefix) + ".mqsad.pk.fold");
  case 9: {
    Value *Accum = emitVectorBuild(B, V4I32, {A, Bv, C, B.CreateXor(A, Bv)});
    Value *R = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_mqsad_u32_u8),
        {packI32PairToI64(B, A, Bv, Twine(NamePrefix) + ".mqsad.a"), C,
         Accum},
        Twine(NamePrefix) + ".mqsad");
    return reduceV4I32ToI32(B, R, Gen, Twine(NamePrefix) + ".mqsad.reduce");
  }
  case 10:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_mul_i24, {I32}),
        {A, Bv}, Twine(NamePrefix) + ".mul.i24");
  case 11:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_mul_u24, {I32}),
        {A, Bv}, Twine(NamePrefix) + ".mul.u24");
  case 12:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_mulhi_i24),
        {A, Bv}, Twine(NamePrefix) + ".mulhi.i24");
  case 13:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_mulhi_u24),
        {A, Bv}, Twine(NamePrefix) + ".mulhi.u24");
  case 14:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_alignbyte),
        {A, Bv, B.CreateAnd(C, ci32(Ctx, 3), Twine(NamePrefix) + ".byte")},
        Twine(NamePrefix) + ".alignbyte");
  case 15:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sffbh, {I32}),
        {A}, Twine(NamePrefix) + ".sffbh");
  case 16:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_mbcnt_lo),
        {A, Bv}, Twine(NamePrefix) + ".mbcnt.lo");
  case 17:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_mbcnt_hi),
        {A, Bv}, Twine(NamePrefix) + ".mbcnt.hi");
  case 18:
    return B.CreateTrunc(B.CreateCall(Intrinsic::getOrInsertDeclaration(
                                          &M, Intrinsic::amdgcn_mul_i24, {I64}),
                                      {A, Bv},
                                      Twine(NamePrefix) + ".mul.i24.i64"),
                         I32, Twine(NamePrefix) + ".mul.i24.i64.trunc");
  case 19:
    return B.CreateTrunc(B.CreateCall(Intrinsic::getOrInsertDeclaration(
                                          &M, Intrinsic::amdgcn_mul_u24, {I64}),
                                      {A, Bv},
                                      Twine(NamePrefix) + ".mul.u24.i64"),
                         I32, Twine(NamePrefix) + ".mul.u24.i64.trunc");
  case 20:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_perm),
        {A, Bv, C}, Twine(NamePrefix) + ".perm");
  case 21:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_bitop3, {I32}),
        {A, Bv, C, ci32(Ctx, Gen() & 255u)},
        Twine(NamePrefix) + ".bitop3");
  case 22:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sdot2),
        {packI32ToV2I16(B, A, Twine(NamePrefix) + ".sdot2.a"),
         packI32ToV2I16(B, Bv, Twine(NamePrefix) + ".sdot2.b"), C, Clamp},
        Twine(NamePrefix) + ".sdot2");
  case 23:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_udot2),
        {packI32ToV2I16(B, A, Twine(NamePrefix) + ".udot2.a"),
         packI32ToV2I16(B, Bv, Twine(NamePrefix) + ".udot2.b"), C, Clamp},
        Twine(NamePrefix) + ".udot2");
  case 24:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sdot4),
        {A, Bv, C, Clamp}, Twine(NamePrefix) + ".sdot4");
  case 25:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_udot4),
        {A, Bv, C, Clamp}, Twine(NamePrefix) + ".udot4");
  case 26:
    if (AllowSUDot)
      return B.CreateCall(
          Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sudot4),
          {ConstantInt::get(I1, Gen() & 1), A,
           ConstantInt::get(I1, Gen() & 1), Bv, C, Clamp},
          Twine(NamePrefix) + ".sudot4");
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sdot8),
        {A, Bv, C, Clamp}, Twine(NamePrefix) + ".sdot8");
  case 27:
    if (!AllowSUDot)
      return B.CreateCall(
          Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_udot8),
          {A, Bv, C, Clamp}, Twine(NamePrefix) + ".udot8");
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sdot8),
        {A, Bv, C, Clamp}, Twine(NamePrefix) + ".sdot8");
  case 28:
  case 29:
  case 30:
  case 31:
  case 32:
  case 33:
  case 34:
  case 35:
  case 36:
  case 37:
    return emitRandomAMDGPUFPIntrinsicInstruction(B, M, A, Bv, Gen,
                                                  NamePrefix);
  case 38:
  case 39:
  case 40:
  case 41:
  case 42:
  case 43:
  case 44:
  case 45:
  case 46:
  case 47:
  case 48:
  case 49:
    return emitRandomAMDGPUWaveInstruction(B, M, A, Bv, Gen, NamePrefix);
  case 50:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sudot4),
        {ConstantInt::get(I1, Gen() & 1), A,
         ConstantInt::get(I1, Gen() & 1), Bv, C, Clamp},
        Twine(NamePrefix) + ".sudot4");
  default:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::amdgcn_sudot8),
        {ConstantInt::get(I1, Gen() & 1), A, ConstantInt::get(I1, Gen() & 1),
         Bv, C, Clamp},
        Twine(NamePrefix) + ".sudot8");
  }
}

Value *emitSafeSignedDivRemInstruction(IRBuilder<NoFolder> &B, Value *A,
                                       Value *Bv, bool IsRem,
                                       StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *Num = A;
  if (!envFlag("FUZZX_ALLOW_M040_SIGNED_DIVREM24", false))
    Num = B.CreateAnd(A, ci32(Ctx, 0x7fffff),
                      Twine(NamePrefix) + ".num.mask");
  Value *DenMask =
      B.CreateAnd(Bv, ci32(Ctx, 255), Twine(NamePrefix) + ".den.mask");
  Value *Den = B.CreateOr(DenMask, ci32(Ctx, 1), Twine(NamePrefix) + ".den");
  if (IsRem)
    return B.CreateSRem(Num, Den, Twine(NamePrefix) + ".op");
  return B.CreateSDiv(Num, Den, Twine(NamePrefix) + ".op");
}

Value *emitSafeInputLoadInstruction(IRBuilder<NoFolder> &B, Module &M,
                                    Value *A, Value *Bv,
                                    std::minstd_rand &Gen,
                                    StringRef NamePrefix) {
  Function *F = B.GetInsertBlock()->getParent();
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Value *In = F->getArg(0);
  Value *N = F->getArg(2);
  Value *Seed = nullptr;
  switch (Gen() % 4) {
  case 0:
    Seed = A;
    break;
  case 1:
    Seed = Bv;
    break;
  case 2:
    Seed = B.CreateAdd(A, Bv, Twine(NamePrefix) + ".seed.add");
    break;
  default:
    Seed = B.CreateXor(A, Bv, Twine(NamePrefix) + ".seed.xor");
    break;
  }

  Value *Idx = B.CreateURem(Seed, N, Twine(NamePrefix) + ".idx");
  Value *Idx64 = B.CreateZExt(Idx, I64, Twine(NamePrefix) + ".idx64");
  Value *Ptr = B.CreateGEP(I32, In, Idx64, Twine(NamePrefix) + ".ptr");
  Value *Loaded = B.CreateLoad(I32, Ptr, Twine(NamePrefix) + ".value");
  switch (Gen() % 4) {
  case 0:
    return B.CreateXor(A, Loaded, Twine(NamePrefix) + ".xor");
  case 1:
    return B.CreateAdd(Bv, Loaded, Twine(NamePrefix) + ".add");
  case 2:
    return B.CreateSub(Loaded, A, Twine(NamePrefix) + ".sub");
  default: {
    Value *Cmp = B.CreateICmp(randomICmpPredicate(Gen), Loaded, A,
                              Twine(NamePrefix) + ".cmp");
    return B.CreateSelect(Cmp, Loaded, Bv, Twine(NamePrefix) + ".select");
  }
  }
}

Value *emitRandomMaskedShiftInstruction(IRBuilder<NoFolder> &B, Value *A,
                                        Value *Bv, std::minstd_rand &Gen,
                                        StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *Shift =
      B.CreateAnd(Bv, ci32(Ctx, 31), Twine(NamePrefix) + ".shift.mask");
  Value *Result = nullptr;
  switch (Gen() % 6) {
  case 0:
    Result = B.CreateShl(A, Shift, Twine(NamePrefix) + ".shl");
    break;
  case 1:
    Result = B.CreateLShr(A, Shift, Twine(NamePrefix) + ".lshr");
    break;
  case 2:
    Result = B.CreateAShr(A, Shift, Twine(NamePrefix) + ".ashr");
    break;
  case 3: {
    Value *Lo = B.CreateLShr(A, Shift, Twine(NamePrefix) + ".lo");
    Value *Hi = B.CreateShl(Bv, Shift, Twine(NamePrefix) + ".hi");
    Result = B.CreateOr(Lo, Hi, Twine(NamePrefix) + ".or");
    break;
  }
  case 4: {
    Value *Lo = B.CreateShl(A, Shift, Twine(NamePrefix) + ".lo");
    Value *Hi = B.CreateLShr(Bv, Shift, Twine(NamePrefix) + ".hi");
    Result = B.CreateXor(Lo, Hi, Twine(NamePrefix) + ".xor");
    break;
  }
  default: {
    Value *Signed = B.CreateAShr(A, Shift, Twine(NamePrefix) + ".signed");
    Value *Unsigned = B.CreateLShr(A, Shift, Twine(NamePrefix) + ".unsigned");
    Value *UseSigned =
        B.CreateICmpSLT(A, ci32(Ctx, 0), Twine(NamePrefix) + ".sign");
    Result = B.CreateSelect(UseSigned, Signed, Unsigned,
                            Twine(NamePrefix) + ".select");
    break;
  }
  }

  switch (Gen() % 4) {
  case 0:
    return Result;
  case 1:
    return B.CreateAdd(Result, A, Twine(NamePrefix) + ".add");
  case 2:
    return B.CreateXor(Result, Bv, Twine(NamePrefix) + ".xor.mix");
  default:
    return B.CreateSub(Bv, Result, Twine(NamePrefix) + ".sub");
  }
}

Value *emitRandomCarryChainIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                 std::minstd_rand &Gen,
                                 StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *C = interestingI32(Ctx, Gen);
  Value *NotA =
      B.CreateXor(A, ci32(Ctx, 0xffffffffu), Twine(NamePrefix) + ".not.a");
  Value *NotB =
      B.CreateXor(Bv, ci32(Ctx, 0xffffffffu), Twine(NamePrefix) + ".not.b");
  switch (Gen() % 7) {
  case 0: {
    Value *Sum = B.CreateXor(A, Bv, Twine(NamePrefix) + ".sum.bits");
    Value *Carry = B.CreateAnd(A, Bv, Twine(NamePrefix) + ".carry.bits");
    Carry = B.CreateShl(Carry, ci32(Ctx, 1), Twine(NamePrefix) + ".carry.shl");
    return B.CreateAdd(Sum, Carry, Twine(NamePrefix) + ".carry.add");
  }
  case 1: {
    Value *Diff = B.CreateXor(A, Bv, Twine(NamePrefix) + ".diff.bits");
    Value *Borrow = B.CreateAnd(NotA, Bv, Twine(NamePrefix) + ".borrow.bits");
    Borrow =
        B.CreateShl(Borrow, ci32(Ctx, 1), Twine(NamePrefix) + ".borrow.shl");
    return B.CreateSub(Diff, Borrow, Twine(NamePrefix) + ".borrow.sub");
  }
  case 2: {
    Value *Parity = B.CreateXor(B.CreateXor(A, Bv, Twine(NamePrefix) + ".pab"),
                                C, Twine(NamePrefix) + ".parity");
    Value *AB = B.CreateAnd(A, Bv, Twine(NamePrefix) + ".ab");
    Value *AC = B.CreateAnd(A, C, Twine(NamePrefix) + ".ac");
    Value *BC = B.CreateAnd(Bv, C, Twine(NamePrefix) + ".bc");
    Value *Majority =
        B.CreateOr(B.CreateOr(AB, AC, Twine(NamePrefix) + ".maj0"), BC,
                   Twine(NamePrefix) + ".majority");
    Majority =
        B.CreateShl(Majority, ci32(Ctx, 1), Twine(NamePrefix) + ".maj.shl");
    return B.CreateXor(Parity, Majority, Twine(NamePrefix) + ".fulladder.mix");
  }
  case 3: {
    Value *Mask = B.CreateAnd(A, C, Twine(NamePrefix) + ".mask.seed");
    Value *KeepA = B.CreateAnd(A, Mask, Twine(NamePrefix) + ".keep.a");
    Value *KeepB = B.CreateAnd(Bv, B.CreateXor(Mask, ci32(Ctx, 0xffffffffu),
                                               Twine(NamePrefix) + ".mask.not"),
                               Twine(NamePrefix) + ".keep.b");
    return B.CreateOr(KeepA, KeepB, Twine(NamePrefix) + ".masked.merge");
  }
  case 4: {
    Value *Generate = B.CreateAnd(A, Bv, Twine(NamePrefix) + ".generate");
    Value *Propagate = B.CreateOr(A, Bv, Twine(NamePrefix) + ".propagate");
    Value *Carry2 =
        B.CreateAnd(Propagate, B.CreateShl(Generate, ci32(Ctx, 1),
                                           Twine(NamePrefix) + ".generate.shl"),
                    Twine(NamePrefix) + ".carry2");
    return B.CreateAdd(B.CreateXor(A, Bv, Twine(NamePrefix) + ".prop.xor"),
                       Carry2, Twine(NamePrefix) + ".prop.add");
  }
  case 5: {
    Value *NoBorrow =
        B.CreateOr(A, NotB, Twine(NamePrefix) + ".noborrow.bits");
    Value *Shifted =
        B.CreateLShr(NoBorrow, ci32(Ctx, 1), Twine(NamePrefix) + ".noborrow.shr");
    return B.CreateXor(Shifted, B.CreateSub(A, Bv, Twine(NamePrefix) + ".sub"),
                       Twine(NamePrefix) + ".noborrow.mix");
  }
  default: {
    Value *Lo = B.CreateAnd(A, ci32(Ctx, 0x55555555u), Twine(NamePrefix) + ".lo");
    Value *Hi =
        B.CreateAnd(Bv, ci32(Ctx, 0xaaaaaaaau), Twine(NamePrefix) + ".hi");
    Value *Cross = B.CreateXor(B.CreateLShr(A, ci32(Ctx, 1),
                                            Twine(NamePrefix) + ".a.shr"),
                               B.CreateShl(Bv, ci32(Ctx, 1),
                                           Twine(NamePrefix) + ".b.shl"),
                               Twine(NamePrefix) + ".cross");
    return B.CreateAdd(B.CreateOr(Lo, Hi, Twine(NamePrefix) + ".lohi"), Cross,
                       Twine(NamePrefix) + ".lohi.add");
  }
  }
}

Value *extractDynamicByteAsI32(IRBuilder<NoFolder> &B, Value *V, Value *Index,
                               const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Value *Lane = B.CreateAnd(Index, ci32(Ctx, 3), Name + ".lane");
  Value *Shift = B.CreateShl(Lane, ci32(Ctx, 3), Name + ".shift");
  Value *Shifted = B.CreateLShr(V, Shift, Name + ".shr");
  return B.CreateAnd(Shifted, ci32(Ctx, 0xff), Name + ".byte");
}

Value *emitRandomDynamicByteIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                  std::minstd_rand &Gen,
                                  StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *Index = (Gen() & 1u) ? A : Bv;
  Value *ByteA = extractDynamicByteAsI32(B, A, Index, Twine(NamePrefix) + ".a");
  Value *NextIndex =
      B.CreateAdd(Index, ci32(Ctx, 1 + (Gen() & 3u)), Twine(NamePrefix) + ".next");
  Value *ByteB =
      extractDynamicByteAsI32(B, Bv, NextIndex, Twine(NamePrefix) + ".b");

  switch (Gen() % 6) {
  case 0:
    return packFourBytesAsI32(
        B, {ByteA, ByteB, B.CreateXor(ByteA, ByteB, Twine(NamePrefix) + ".xor"),
            B.CreateAdd(ByteA, ByteB, Twine(NamePrefix) + ".add")},
        (Gen() & 1u) != 0, Twine(NamePrefix) + ".pack");
  case 1: {
    Value *Cmp = B.CreateICmpUGT(ByteA, ByteB, Twine(NamePrefix) + ".cmp");
    return B.CreateSelect(Cmp, ByteA, ByteB, Twine(NamePrefix) + ".select");
  }
  case 2: {
    Value *Lo = B.CreateOr(ByteA, B.CreateShl(ByteB, ci32(Ctx, 8),
                                             Twine(NamePrefix) + ".b.shift"),
                           Twine(NamePrefix) + ".lo16");
    Value *Hi = extractHalfAsI32(B, (Gen() & 1u) ? A : Bv, 1, false,
                                 Twine(NamePrefix) + ".hi16");
    Value *HiShift = B.CreateShl(B.CreateAnd(Hi, ci32(Ctx, 0xffff),
                                             Twine(NamePrefix) + ".hi.mask"),
                                 ci32(Ctx, 16),
                                 Twine(NamePrefix) + ".hi.shift");
    return B.CreateOr(B.CreateAnd(Lo, ci32(Ctx, 0xffff),
                                  Twine(NamePrefix) + ".lo.mask"),
                      HiShift, Twine(NamePrefix) + ".halves");
  }
  case 3: {
    Value *RotIndex =
        B.CreateXor(Index, ci32(Ctx, 2), Twine(NamePrefix) + ".rot.index");
    Value *ByteC =
        extractDynamicByteAsI32(B, A, RotIndex, Twine(NamePrefix) + ".c");
    return B.CreateSub(B.CreateAdd(ByteA, ByteC, Twine(NamePrefix) + ".sum"),
                       ByteB, Twine(NamePrefix) + ".sub");
  }
  case 4: {
    Value *Mask =
        B.CreateSelect(B.CreateICmpULT(ByteA, ByteB, Twine(NamePrefix) + ".lt"),
                       ci32(Ctx, 0xff), ci32(Ctx, 0), Twine(NamePrefix) + ".mask");
    return B.CreateOr(B.CreateAnd(ByteA, Mask, Twine(NamePrefix) + ".masked.a"),
                      B.CreateAnd(ByteB,
                                  B.CreateXor(Mask, ci32(Ctx, 0xff),
                                              Twine(NamePrefix) + ".mask.not"),
                                  Twine(NamePrefix) + ".masked.b"),
                      Twine(NamePrefix) + ".blend");
  }
  default:
    return B.CreateXor(B.CreateMul(ByteA, ByteB, Twine(NamePrefix) + ".mul"),
                       B.CreateAdd(A, Bv, Twine(NamePrefix) + ".wide.add"),
                       Twine(NamePrefix) + ".mul.mix");
  }
}

Value *emitRandomCompareRankIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                  std::minstd_rand &Gen,
                                  StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *C = interestingI32(Ctx, Gen);
  Value *P0 = B.CreateICmpULT(A, Bv, Twine(NamePrefix) + ".ult");
  Value *P1 = B.CreateICmpSGT(A, C, Twine(NamePrefix) + ".sgt");
  Value *P2 = B.CreateICmpEQ(B.CreateAnd(A, ci32(Ctx, 0xff),
                                         Twine(NamePrefix) + ".a.byte"),
                             B.CreateAnd(Bv, ci32(Ctx, 0xff),
                                         Twine(NamePrefix) + ".b.byte"),
                             Twine(NamePrefix) + ".byte.eq");
  Value *Z0 = B.CreateZExt(P0, I32, Twine(NamePrefix) + ".z0");
  Value *Z1 = B.CreateZExt(P1, I32, Twine(NamePrefix) + ".z1");
  Value *Z2 = B.CreateZExt(P2, I32, Twine(NamePrefix) + ".z2");
  Value *Rank =
      B.CreateAdd(Z0, B.CreateShl(Z1, ci32(Ctx, 1), Twine(NamePrefix) + ".z1.shl"),
                  Twine(NamePrefix) + ".rank01");
  Rank = B.CreateAdd(Rank, B.CreateShl(Z2, ci32(Ctx, 2),
                                       Twine(NamePrefix) + ".z2.shl"),
                     Twine(NamePrefix) + ".rank");
  switch (Gen() % 5) {
  case 0:
    return Rank;
  case 1:
    return B.CreateSelect(P0, B.CreateXor(A, Rank, Twine(NamePrefix) + ".a.xor"),
                          B.CreateAdd(Bv, Rank, Twine(NamePrefix) + ".b.add"),
                          Twine(NamePrefix) + ".select");
  case 2: {
    Value *Mask = buildPredicateMask(B, P1, Gen, Twine(NamePrefix) + ".mask");
    return B.CreateOr(B.CreateAnd(A, Mask, Twine(NamePrefix) + ".mask.a"),
                      B.CreateAnd(Bv,
                                  B.CreateXor(Mask, ci32(Ctx, 0xffffffffu),
                                              Twine(NamePrefix) + ".mask.not"),
                                  Twine(NamePrefix) + ".mask.b"),
                      Twine(NamePrefix) + ".mask.merge");
  }
  case 3:
    return B.CreateSub(B.CreateAdd(A, Rank, Twine(NamePrefix) + ".a.rank"),
                       B.CreateZExt(B.CreateXor(P1, P2, Twine(NamePrefix) + ".pxor"),
                                    I32, Twine(NamePrefix) + ".pxor.z"),
                       Twine(NamePrefix) + ".sub");
  default:
    return B.CreateXor(B.CreateShl(Rank, ci32(Ctx, 28),
                                   Twine(NamePrefix) + ".rank.high"),
                       B.CreateLShr((Gen() & 1u) ? A : Bv, ci32(Ctx, 4),
                                    Twine(NamePrefix) + ".shr"),
                       Twine(NamePrefix) + ".mix");
  }
}

Value *emitRandomNarrowScalarInstruction(IRBuilder<NoFolder> &B, Module &M,
                                         Value *A, Value *Bv,
                                         std::minstd_rand &Gen,
                                         StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *NarrowTy =
      (Gen() % 2) == 0 ? Type::getInt8Ty(Ctx) : Type::getInt16Ty(Ctx);
  unsigned Width = cast<IntegerType>(NarrowTy)->getBitWidth();
  Value *NA = B.CreateTrunc(A, NarrowTy, Twine(NamePrefix) + ".trunc.a");
  Value *NBSeed = nullptr;
  if ((Gen() % 3) == 0)
    NBSeed = ConstantInt::get(NarrowTy, randomInteresting64(Gen));
  else
    NBSeed = B.CreateTrunc(Bv, NarrowTy, Twine(NamePrefix) + ".trunc.b");

  Value *Result = nullptr;
  switch (Gen() % 29) {
  case 0:
    Result = B.CreateAdd(NA, NBSeed, Twine(NamePrefix) + ".add");
    break;
  case 1:
    Result = B.CreateSub(NA, NBSeed, Twine(NamePrefix) + ".sub");
    break;
  case 2:
    Result = B.CreateMul(NA, NBSeed, Twine(NamePrefix) + ".mul");
    break;
  case 3:
    Result = B.CreateXor(NA, NBSeed, Twine(NamePrefix) + ".xor");
    break;
  case 4:
    Result = B.CreateAnd(NA, NBSeed, Twine(NamePrefix) + ".and");
    break;
  case 5:
    Result = B.CreateOr(NA, NBSeed, Twine(NamePrefix) + ".or");
    break;
  case 6:
    Result = B.CreateShl(NA, ConstantInt::get(NarrowTy, Gen() % Width),
                         Twine(NamePrefix) + ".shl");
    break;
  case 7:
    Result = B.CreateLShr(NA, ConstantInt::get(NarrowTy, Gen() % Width),
                          Twine(NamePrefix) + ".lshr");
    break;
  case 8:
    Result = B.CreateAShr(NA, ConstantInt::get(NarrowTy, Gen() % Width),
                          Twine(NamePrefix) + ".ashr");
    break;
  case 9:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctpop, {NarrowTy}),
        {NA}, Twine(NamePrefix) + ".ctpop");
    break;
  case 10:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bitreverse,
                                          {NarrowTy}),
        {NA}, Twine(NamePrefix) + ".bitreverse");
    break;
  case 11:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctlz, {NarrowTy}),
        {NA, ConstantInt::getFalse(Ctx)}, Twine(NamePrefix) + ".ctlz");
    break;
  case 12:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::cttz, {NarrowTy}),
        {NA, ConstantInt::getFalse(Ctx)}, Twine(NamePrefix) + ".cttz");
    break;
  case 13:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::abs, {NarrowTy}),
        {NA, ConstantInt::getFalse(Ctx)}, Twine(NamePrefix) + ".abs");
    break;
  case 14:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umin, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".umin");
    break;
  case 15:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umax, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".umax");
    break;
  case 16:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smin, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".smin");
    break;
  case 17:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smax, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".smax");
    break;
  case 18:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::uadd_sat, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".uadd_sat");
    break;
  case 19:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::usub_sat, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".usub_sat");
    break;
  case 20:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sadd_sat, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".sadd_sat");
    break;
  case 21:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ssub_sat, {NarrowTy}),
        {NA, NBSeed}, Twine(NamePrefix) + ".ssub_sat");
    break;
  case 22:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {NarrowTy}),
        {NA, NBSeed, ConstantInt::get(NarrowTy, Gen() % Width)},
        Twine(NamePrefix) + ".fshl");
    break;
  case 23:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {NarrowTy}),
        {NA, NBSeed, ConstantInt::get(NarrowTy, Gen() % Width)},
        Twine(NamePrefix) + ".fshr");
    break;
  case 24:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {NarrowTy}),
        {NA, NBSeed, NBSeed}, Twine(NamePrefix) + ".fshl.dyn");
    break;
  case 25:
    Result = B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {NarrowTy}),
        {NA, NBSeed, NA}, Twine(NamePrefix) + ".fshr.dyn");
    break;
  case 26: {
    Value *Cmp =
        B.CreateICmp(randomICmpPredicate(Gen), NA, NBSeed,
                     Twine(NamePrefix) + ".cmp");
    Result = B.CreateSelect(Cmp, NA, NBSeed, Twine(NamePrefix) + ".select");
    break;
  }
  case 27: {
    Value *Den =
        B.CreateOr(NBSeed, ConstantInt::get(NarrowTy, 1),
                   Twine(NamePrefix) + ".nz");
    Result = B.CreateUDiv(NA, Den, Twine(NamePrefix) + ".udiv");
    break;
  }
  default: {
    Value *Den =
        B.CreateOr(NBSeed, ConstantInt::get(NarrowTy, 1),
                   Twine(NamePrefix) + ".nz");
    Result = B.CreateURem(NA, Den, Twine(NamePrefix) + ".urem");
    break;
  }
  }

  Value *Ext = (Gen() % 2) == 0
                   ? B.CreateZExt(Result, I32, Twine(NamePrefix) + ".zext")
                   : B.CreateSExt(Result, I32, Twine(NamePrefix) + ".sext");
  switch (Gen() % 4) {
  case 0:
    return Ext;
  case 1:
    return B.CreateXor(Ext, A, Twine(NamePrefix) + ".xor.i32");
  case 2:
    return B.CreateAdd(Ext, Bv, Twine(NamePrefix) + ".add.i32");
  default:
    return B.CreateSub(A, Ext, Twine(NamePrefix) + ".sub.i32");
  }
}

Value *unsignedMinMaxSelect(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                            bool WantMax, const Twine &Name) {
  Value *Cmp = B.CreateICmpUGT(A, Bv, Name + ".cmp");
  return WantMax ? B.CreateSelect(Cmp, A, Bv, Name + ".umax")
                 : B.CreateSelect(Cmp, Bv, A, Name + ".umin");
}

Value *signedMinMaxSelect(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                          bool WantMax, const Twine &Name) {
  Value *Cmp = B.CreateICmpSGT(A, Bv, Name + ".cmp");
  return WantMax ? B.CreateSelect(Cmp, A, Bv, Name + ".smax")
                 : B.CreateSelect(Cmp, Bv, A, Name + ".smin");
}

Value *unsignedAbsDiffI32(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                          const Twine &Name) {
  Value *Hi = unsignedMinMaxSelect(B, A, Bv, true, Name + ".hi");
  Value *Lo = unsignedMinMaxSelect(B, A, Bv, false, Name + ".lo");
  return B.CreateSub(Hi, Lo, Name + ".absdiff");
}

Value *emitRandomAverageDiffIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                  std::minstd_rand &Gen,
                                  StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);

  switch (Gen() % 8) {
  case 0: {
    Value *Shared = B.CreateAnd(A, Bv, Twine(NamePrefix) + ".avg.and");
    Value *Diff = B.CreateXor(A, Bv, Twine(NamePrefix) + ".avg.xor");
    Value *Half = B.CreateLShr(Diff, ci32(Ctx, 1),
                               Twine(NamePrefix) + ".avg.half");
    return B.CreateAdd(Shared, Half, Twine(NamePrefix) + ".avg.floor");
  }
  case 1: {
    Value *Either = B.CreateOr(A, Bv, Twine(NamePrefix) + ".avg.or");
    Value *Diff = B.CreateXor(A, Bv, Twine(NamePrefix) + ".avg.xor");
    Value *Half = B.CreateLShr(Diff, ci32(Ctx, 1),
                               Twine(NamePrefix) + ".avg.half");
    return B.CreateSub(Either, Half, Twine(NamePrefix) + ".avg.ceil");
  }
  case 2: {
    Value *Sum = B.CreateAdd(B.CreateZExt(A, I64, Twine(NamePrefix) + ".a64"),
                             B.CreateZExt(Bv, I64, Twine(NamePrefix) + ".b64"),
                             Twine(NamePrefix) + ".sum64");
    Sum = B.CreateAdd(Sum, ConstantInt::get(I64, 1),
                      Twine(NamePrefix) + ".sum64.round");
    Value *Avg = B.CreateLShr(Sum, ConstantInt::get(I64, 1),
                              Twine(NamePrefix) + ".avg64");
    return B.CreateTrunc(Avg, I32, Twine(NamePrefix) + ".avg.round");
  }
  case 3:
    return unsignedAbsDiffI32(B, A, Bv, Twine(NamePrefix) + ".u32");
  case 4: {
    Value *Lo = unsignedMinMaxSelect(B, A, Bv, false,
                                     Twine(NamePrefix) + ".u32.lo");
    Value *Hi = unsignedMinMaxSelect(B, A, Bv, true,
                                     Twine(NamePrefix) + ".u32.hi");
    Value *Distance = B.CreateSub(Hi, Lo, Twine(NamePrefix) + ".u32.dist");
    Value *Mid = B.CreateAdd(
        Lo, B.CreateLShr(Distance, ci32(Ctx, 1),
                         Twine(NamePrefix) + ".u32.dist.half"),
        Twine(NamePrefix) + ".u32.mid");
    return B.CreateXor(Mid, Distance, Twine(NamePrefix) + ".u32.mid.mix");
  }
  case 5: {
    Value *Sum = ci32(Ctx, 0);
    for (unsigned Byte = 0; Byte != 4; ++Byte) {
      Value *AB = extractByteAsI32(B, A, Byte,
                                   Twine(NamePrefix) + ".byte.a");
      Value *BB = extractByteAsI32(B, Bv, Byte,
                                   Twine(NamePrefix) + ".byte.b");
      Value *Delta =
          unsignedAbsDiffI32(B, AB, BB, Twine(NamePrefix) + ".byte.delta");
      Sum = B.CreateAdd(Sum, Delta, Twine(NamePrefix) + ".byte.sum");
    }
    return Sum;
  }
  case 6: {
    Value *LoA = extractHalfAsI32(B, A, 0, false, Twine(NamePrefix) + ".lo.a");
    Value *LoB =
        extractHalfAsI32(B, Bv, 0, false, Twine(NamePrefix) + ".lo.b");
    Value *HiA = extractHalfAsI32(B, A, 1, false, Twine(NamePrefix) + ".hi.a");
    Value *HiB =
        extractHalfAsI32(B, Bv, 1, false, Twine(NamePrefix) + ".hi.b");
    Value *LoDelta =
        unsignedAbsDiffI32(B, LoA, LoB, Twine(NamePrefix) + ".half.lo");
    Value *HiDelta =
        unsignedAbsDiffI32(B, HiA, HiB, Twine(NamePrefix) + ".half.hi");
    return B.CreateAdd(LoDelta, HiDelta, Twine(NamePrefix) + ".half.sum");
  }
  default: {
    Value *ASmall = B.CreateSExt(B.CreateTrunc(A, I16,
                                               Twine(NamePrefix) + ".a16"),
                                 I64, Twine(NamePrefix) + ".a64.s");
    Value *BSmall = B.CreateSExt(B.CreateTrunc(Bv, I16,
                                               Twine(NamePrefix) + ".b16"),
                                 I64, Twine(NamePrefix) + ".b64.s");
    Value *Lo = signedMinMaxSelect(B, ASmall, BSmall, false,
                                   Twine(NamePrefix) + ".s16.lo");
    Value *Hi = signedMinMaxSelect(B, ASmall, BSmall, true,
                                   Twine(NamePrefix) + ".s16.hi");
    Value *Distance = B.CreateSub(Hi, Lo, Twine(NamePrefix) + ".s16.dist");
    Value *Mid =
        B.CreateAdd(Lo,
                    B.CreateAShr(Distance, ConstantInt::get(I64, 1),
                                 Twine(NamePrefix) + ".s16.dist.half"),
                    Twine(NamePrefix) + ".s16.mid");
    return B.CreateTrunc(Mid, I32, Twine(NamePrefix) + ".s16.mid.i32");
  }
  }
}

Value *emitRandomByteDotChainIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                   std::minstd_rand &Gen,
                                   StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  SmallVector<Value *, 4> ProductBytes;
  Value *Acc = ci32(Ctx, Gen() & 0xffu);

  for (unsigned I = 0; I != 4; ++I) {
    Value *LhsSrc = ((Gen() + I) & 1u) ? A : Bv;
    Value *RhsSrc = ((Gen() + I) & 1u) ? Bv : A;
    unsigned LhsByte = (I + Gen()) & 3u;
    unsigned RhsByte = ((3u - I) + Gen()) & 3u;
    bool SignedLhs = (Gen() & 3u) == 0;
    bool SignedRhs = (Gen() & 3u) == 0;
    Value *Lhs = SignedLhs
                     ? extractSignedByteAsI32(
                           B, LhsSrc, LhsByte,
                           Twine(NamePrefix) + ".lhs.sbyte")
                     : extractByteAsI32(B, LhsSrc, LhsByte,
                                        Twine(NamePrefix) + ".lhs.ubyte");
    Value *Rhs = SignedRhs
                     ? extractSignedByteAsI32(
                           B, RhsSrc, RhsByte,
                           Twine(NamePrefix) + ".rhs.sbyte")
                     : extractByteAsI32(B, RhsSrc, RhsByte,
                                        Twine(NamePrefix) + ".rhs.ubyte");
    Value *Product = B.CreateMul(Lhs, Rhs, Twine(NamePrefix) + ".mul");
    ProductBytes.push_back(Product);

    switch ((Gen() + I) % 5) {
    case 0:
      Acc = B.CreateAdd(Acc, Product, Twine(NamePrefix) + ".acc.add");
      break;
    case 1:
      Acc = B.CreateSub(Acc, Product, Twine(NamePrefix) + ".acc.sub");
      break;
    case 2:
      Acc = B.CreateXor(Acc, Product, Twine(NamePrefix) + ".acc.xor");
      break;
    case 3: {
      Value *Low = B.CreateAnd(Product, ci32(Ctx, 0xffff),
                               Twine(NamePrefix) + ".mul.low");
      Acc = B.CreateAdd(Acc, Low, Twine(NamePrefix) + ".acc.add.low");
      break;
    }
    default: {
      Value *Shifted =
          B.CreateShl(B.CreateAnd(Product, ci32(Ctx, 0xff),
                                  Twine(NamePrefix) + ".mul.byte"),
                      ci32(Ctx, (I & 3u) * 4u),
                      Twine(NamePrefix) + ".mul.byte.shift");
      Acc = B.CreateXor(Acc, Shifted, Twine(NamePrefix) + ".acc.xor.shift");
      break;
    }
    }
  }

  Value *Packed = packFourBytesAsI32(B, ProductBytes, (Gen() & 1u) != 0,
                                     Twine(NamePrefix) + ".pack.products");
  switch (Gen() % 5) {
  case 0:
    return Acc;
  case 1:
    return B.CreateAdd(Acc, Packed, Twine(NamePrefix) + ".result.add");
  case 2:
    return B.CreateXor(Acc, Packed, Twine(NamePrefix) + ".result.xor");
  case 3: {
    Value *Byte0 = extractByteAsI32(B, Packed, Gen() & 3u,
                                    Twine(NamePrefix) + ".packed.byte");
    return B.CreateSub(Acc, Byte0, Twine(NamePrefix) + ".result.sub.byte");
  }
  default: {
    Value *Cmp = B.CreateICmpUGT(Acc, Packed, Twine(NamePrefix) + ".cmp");
    return B.CreateSelect(Cmp, Acc, Packed, Twine(NamePrefix) + ".select");
  }
  }
}

Value *unsignedClampI32(IRBuilder<NoFolder> &B, Value *V, uint32_t Lo,
                        uint32_t Hi, const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Value *LoC = ci32(Ctx, Lo);
  Value *HiC = ci32(Ctx, Hi);
  Value *Below = B.CreateICmpULT(V, LoC, Name + ".below");
  Value *AtLeastLo = B.CreateSelect(Below, LoC, V, Name + ".atleast");
  Value *Above = B.CreateICmpUGT(AtLeastLo, HiC, Name + ".above");
  return B.CreateSelect(Above, HiC, AtLeastLo, Name + ".clamp");
}

Value *signedClampI32(IRBuilder<NoFolder> &B, Value *V, int32_t Lo,
                      int32_t Hi, const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *LoC = ConstantInt::get(I32, Lo, true);
  Value *HiC = ConstantInt::get(I32, Hi, true);
  Value *Below = B.CreateICmpSLT(V, LoC, Name + ".below");
  Value *AtLeastLo = B.CreateSelect(Below, LoC, V, Name + ".atleast");
  Value *Above = B.CreateICmpSGT(AtLeastLo, HiC, Name + ".above");
  return B.CreateSelect(Above, HiC, AtLeastLo, Name + ".clamp");
}

Value *packTwoHalvesAsI32(IRBuilder<NoFolder> &B, Value *Lo, Value *Hi,
                          bool UseAdd, const Twine &Name) {
  LLVMContext &Ctx = Lo->getContext();
  Value *LoPart = B.CreateAnd(Lo, ci32(Ctx, 0xffff), Name + ".lo.mask");
  Value *HiPart = B.CreateAnd(Hi, ci32(Ctx, 0xffff), Name + ".hi.mask");
  HiPart = B.CreateShl(HiPart, ci32(Ctx, 16), Name + ".hi.shift");
  return UseAdd ? B.CreateAdd(LoPart, HiPart, Name + ".add")
                : B.CreateOr(LoPart, HiPart, Name + ".or");
}

Value *emitRandomClampPackIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                std::minstd_rand &Gen,
                                StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  switch (Gen() % 8) {
  case 0: {
    static constexpr std::array<uint32_t, 4> Highs = {15, 31, 127, 255};
    SmallVector<Value *, 4> Bytes;
    for (unsigned I = 0; I != 4; ++I) {
      Value *Src = (Gen() & 1u) ? A : Bv;
      Value *Byte =
          extractByteAsI32(B, Src, I, Twine(NamePrefix) + ".u8.src");
      Bytes.push_back(unsignedClampI32(B, Byte, 0, Highs[(I + Gen()) & 3],
                                       Twine(NamePrefix) + ".u8.clamp"));
    }
    return packFourBytesAsI32(B, Bytes, (Gen() & 1u) != 0,
                              Twine(NamePrefix) + ".u8.pack");
  }
  case 1: {
    SmallVector<Value *, 4> Bytes;
    for (unsigned I = 0; I != 4; ++I) {
      Value *Src = (I & 1u) ? Bv : A;
      Value *Half =
          extractHalfAsI32(B, Src, I & 1u, true,
                           Twine(NamePrefix) + ".s16.src");
      Bytes.push_back(signedClampI32(B, Half, -128, 127,
                                     Twine(NamePrefix) + ".s8.clamp"));
    }
    return packFourBytesAsI32(B, Bytes, false,
                              Twine(NamePrefix) + ".s8.pack");
  }
  case 2: {
    SmallVector<Value *, 4> Bytes;
    for (unsigned I = 0; I != 4; ++I) {
      Value *AB = extractByteAsI32(B, A, I, Twine(NamePrefix) + ".add.a");
      Value *BB = extractByteAsI32(B, Bv, I, Twine(NamePrefix) + ".add.b");
      Value *Sum = B.CreateAdd(AB, BB, Twine(NamePrefix) + ".u8.add");
      Bytes.push_back(unsignedClampI32(B, Sum, 0, 255,
                                       Twine(NamePrefix) + ".u8.add.sat"));
    }
    return packFourBytesAsI32(B, Bytes, true,
                              Twine(NamePrefix) + ".u8.add.pack");
  }
  case 3: {
    SmallVector<Value *, 4> Bytes;
    for (unsigned I = 0; I != 4; ++I) {
      Value *AB = extractByteAsI32(B, A, I, Twine(NamePrefix) + ".sub.a");
      Value *BB = extractByteAsI32(B, Bv, I, Twine(NamePrefix) + ".sub.b");
      Value *Diff = B.CreateSub(AB, BB, Twine(NamePrefix) + ".u8.sub");
      Value *Keep = B.CreateICmpUGE(AB, BB, Twine(NamePrefix) + ".u8.sub.ok");
      Bytes.push_back(B.CreateSelect(Keep, Diff, ci32(Ctx, 0),
                                     Twine(NamePrefix) + ".u8.sub.sat"));
    }
    return packFourBytesAsI32(B, Bytes, false,
                              Twine(NamePrefix) + ".u8.sub.pack");
  }
  case 4: {
    Value *LoA = extractHalfAsI32(B, A, 0, false, Twine(NamePrefix) + ".lo.a");
    Value *LoB =
        extractHalfAsI32(B, Bv, 0, false, Twine(NamePrefix) + ".lo.b");
    Value *HiA = extractHalfAsI32(B, A, 1, false, Twine(NamePrefix) + ".hi.a");
    Value *HiB =
        extractHalfAsI32(B, Bv, 1, false, Twine(NamePrefix) + ".hi.b");
    Value *Lo = unsignedClampI32(
        B, B.CreateAdd(LoA, LoB, Twine(NamePrefix) + ".lo.add"), 0, 0xffff,
        Twine(NamePrefix) + ".lo.add.sat");
    Value *Hi = unsignedClampI32(
        B, B.CreateAdd(HiA, HiB, Twine(NamePrefix) + ".hi.add"), 0, 0xffff,
        Twine(NamePrefix) + ".hi.add.sat");
    return packTwoHalvesAsI32(B, Lo, Hi, (Gen() & 1u) != 0,
                              Twine(NamePrefix) + ".u16.add.pack");
  }
  case 5: {
    Value *LoA = extractHalfAsI32(B, A, 0, true, Twine(NamePrefix) + ".lo.a");
    Value *LoB =
        extractHalfAsI32(B, Bv, 0, true, Twine(NamePrefix) + ".lo.b");
    Value *HiA = extractHalfAsI32(B, A, 1, true, Twine(NamePrefix) + ".hi.a");
    Value *HiB =
        extractHalfAsI32(B, Bv, 1, true, Twine(NamePrefix) + ".hi.b");
    Value *Lo = signedClampI32(
        B, B.CreateAdd(LoA, LoB, Twine(NamePrefix) + ".lo.sadd"), -2048, 2047,
        Twine(NamePrefix) + ".lo.sclamp");
    Value *Hi = signedClampI32(
        B, B.CreateSub(HiA, HiB, Twine(NamePrefix) + ".hi.ssub"), -2048, 2047,
        Twine(NamePrefix) + ".hi.sclamp");
    return packTwoHalvesAsI32(B, Lo, Hi, false,
                              Twine(NamePrefix) + ".s16.pack");
  }
  case 6: {
    SmallVector<Value *, 4> Bytes;
    for (unsigned I = 0; I != 4; ++I) {
      Value *AB = extractByteAsI32(B, A, I, Twine(NamePrefix) + ".minmax.a");
      Value *BB = extractByteAsI32(B, Bv, I, Twine(NamePrefix) + ".minmax.b");
      Bytes.push_back(unsignedMinMaxSelect(
          B, AB, BB, (I & 1u) != 0, Twine(NamePrefix) + ".u8.minmax"));
    }
    return packFourBytesAsI32(B, Bytes, (Gen() & 1u) != 0,
                              Twine(NamePrefix) + ".u8.minmax.pack");
  }
  default: {
    SmallVector<Value *, 4> Bytes;
    uint32_t Threshold = (Gen() & 1u) ? 127 : 63;
    for (unsigned I = 0; I != 4; ++I) {
      Value *Byte =
          extractByteAsI32(B, (I & 1u) ? A : Bv, I,
                           Twine(NamePrefix) + ".threshold.src");
      Value *Large = B.CreateICmpUGT(Byte, ci32(Ctx, Threshold),
                                     Twine(NamePrefix) + ".threshold.cmp");
      Value *Clamped = unsignedClampI32(
          B, B.CreateXor(Byte, ci32(Ctx, (I + 1) * 17),
                         Twine(NamePrefix) + ".threshold.xor"),
          0, 255, Twine(NamePrefix) + ".threshold.clamp");
      Bytes.push_back(B.CreateSelect(Large, Clamped, Byte,
                                     Twine(NamePrefix) + ".threshold.select"));
    }
    return packFourBytesAsI32(B, Bytes, true,
                              Twine(NamePrefix) + ".threshold.pack");
  }
  }
}

Value *reduceI32VectorToI32(IRBuilder<NoFolder> &B, Value *V,
                            std::minstd_rand &Gen, const Twine &Name) {
  LLVMContext &Ctx = V->getContext();
  auto *VT = cast<FixedVectorType>(V->getType());
  unsigned Lanes = VT->getNumElements();
  unsigned Mode = Gen() % 7;
  Value *Acc = B.CreateExtractElement(V, ci32(Ctx, 0), Name + ".lane");
  for (unsigned Lane = 1; Lane != Lanes; ++Lane) {
    Value *Elt = B.CreateExtractElement(V, ci32(Ctx, Lane), Name + ".lane");
    switch (Mode) {
    case 0:
      Acc = B.CreateAdd(Acc, Elt, Name + ".add");
      break;
    case 1:
      Acc = B.CreateXor(Acc, Elt, Name + ".xor");
      break;
    case 2:
      Acc = B.CreateOr(Acc, Elt, Name + ".or");
      break;
    case 3:
      Acc = B.CreateAnd(Acc, Elt, Name + ".and");
      break;
    case 4:
      Acc = unsignedMinMaxSelect(B, Acc, Elt, (Gen() & 1u) != 0,
                                 Name + ".uminmax");
      break;
    case 5:
      Acc = signedMinMaxSelect(B, Acc, Elt, (Gen() & 1u) != 0,
                               Name + ".sminmax");
      break;
    default:
      Acc = B.CreateSub(Acc, Elt, Name + ".sub");
      break;
    }
  }
  return Acc;
}

SmallVector<int, 8> rotateShuffleMask(unsigned Lanes, unsigned Amount) {
  SmallVector<int, 8> Mask;
  for (unsigned I = 0; I != Lanes; ++I)
    Mask.push_back((I + Amount) % Lanes);
  return Mask;
}

SmallVector<int, 8> reverseShuffleMask(unsigned Lanes) {
  SmallVector<int, 8> Mask;
  for (unsigned I = 0; I != Lanes; ++I)
    Mask.push_back(Lanes - 1 - I);
  return Mask;
}

Value *emitRandomVectorReductionIdiom(IRBuilder<NoFolder> &B, Value *A,
                                      Value *Bv, std::minstd_rand &Gen,
                                      StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  unsigned Lanes = (Gen() % 2) == 0 ? 2 : 4;
  auto *VecTy = FixedVectorType::get(I32, Lanes);
  SmallVector<Value *, 4> AElements;
  SmallVector<Value *, 4> BElements;
  for (unsigned I = 0; I != Lanes; ++I) {
    Value *BaseA = (I & 1u) ? Bv : A;
    Value *BaseB = (I & 1u) ? A : Bv;
    switch ((Gen() + I) % 5) {
    case 0:
      AElements.push_back(B.CreateAdd(BaseA, ci32(Ctx, I + 1),
                                      Twine(NamePrefix) + ".a.add"));
      BElements.push_back(B.CreateXor(BaseB, ci32(Ctx, 0x10101010u * (I + 1)),
                                      Twine(NamePrefix) + ".b.xor"));
      break;
    case 1:
      AElements.push_back(B.CreateAnd(BaseA, ci32(Ctx, 0xffffu << (I & 1u)),
                                      Twine(NamePrefix) + ".a.mask"));
      BElements.push_back(B.CreateOr(BaseB, ci32(Ctx, (I + 1) * 17),
                                     Twine(NamePrefix) + ".b.or"));
      break;
    case 2:
      AElements.push_back(B.CreateLShr(BaseA, ci32(Ctx, I * 3),
                                       Twine(NamePrefix) + ".a.shr"));
      BElements.push_back(B.CreateShl(BaseB, ci32(Ctx, I + 1),
                                      Twine(NamePrefix) + ".b.shl"));
      break;
    case 3:
      AElements.push_back(unsignedAbsDiffI32(B, BaseA, BaseB,
                                             Twine(NamePrefix) + ".a.absdiff"));
      BElements.push_back(B.CreateSub(BaseB, BaseA,
                                      Twine(NamePrefix) + ".b.sub"));
      break;
    default:
      AElements.push_back(B.CreateXor(BaseA, BaseB,
                                      Twine(NamePrefix) + ".a.xor"));
      BElements.push_back(interestingI32(Ctx, Gen));
      break;
    }
  }

  Value *VA = emitVectorBuild(B, VecTy, AElements);
  Value *VB = emitVectorBuild(B, VecTy, BElements);
  Value *Rot = B.CreateShuffleVector(
      VA, VB, rotateShuffleMask(Lanes, 1 + (Gen() % Lanes)),
      Twine(NamePrefix) + ".rot");
  Value *Rev = B.CreateShuffleVector(VB, VA, reverseShuffleMask(Lanes),
                                     Twine(NamePrefix) + ".rev");
  Value *Mixed = nullptr;
  switch (Gen() % 8) {
  case 0:
    Mixed = B.CreateAdd(VA, Rot, Twine(NamePrefix) + ".vadd");
    break;
  case 1:
    Mixed = B.CreateSub(Rot, VA, Twine(NamePrefix) + ".vsub");
    break;
  case 2:
    Mixed = B.CreateXor(VA, Rev, Twine(NamePrefix) + ".vxor");
    break;
  case 3:
    Mixed = B.CreateOr(B.CreateAnd(VA, Rot, Twine(NamePrefix) + ".vand"),
                       Rev, Twine(NamePrefix) + ".vor");
    break;
  case 4:
    Mixed = unsignedMinMaxSelect(B, VA, Rot, (Gen() & 1u) != 0,
                                 Twine(NamePrefix) + ".vuminmax");
    break;
  case 5:
    Mixed = signedMinMaxSelect(B, VA, Rev, (Gen() & 1u) != 0,
                               Twine(NamePrefix) + ".vsminmax");
    break;
  case 6: {
    Value *Cmp = B.CreateICmpULT(VA, Rot, Twine(NamePrefix) + ".vcmp");
    Mixed = B.CreateSelect(Cmp, Rev, VA, Twine(NamePrefix) + ".vselect");
    break;
  }
  default:
    Mixed = B.CreateLShr(B.CreateAdd(VA, Rev, Twine(NamePrefix) + ".avg.sum"),
                         randomShiftVector(Ctx, Lanes, Gen),
                         Twine(NamePrefix) + ".avg.shift");
    break;
  }

  Value *Reduced =
      reduceI32VectorToI32(B, Mixed, Gen, Twine(NamePrefix) + ".reduce");
  switch (Gen() % 4) {
  case 0:
    return Reduced;
  case 1:
    return B.CreateXor(Reduced, A, Twine(NamePrefix) + ".xor");
  case 2:
    return B.CreateAdd(Reduced, Bv, Twine(NamePrefix) + ".add");
  default:
    return B.CreateSub(A, Reduced, Twine(NamePrefix) + ".sub");
  }
}

Value *emitRandomTernaryBitIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                 std::minstd_rand &Gen,
                                 StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *C = interestingI32(Ctx, Gen);
  Value *NotA =
      B.CreateXor(A, ci32(Ctx, 0xffffffffu), Twine(NamePrefix) + ".not.a");
  Value *NotB =
      B.CreateXor(Bv, ci32(Ctx, 0xffffffffu), Twine(NamePrefix) + ".not.b");
  Value *NotC =
      B.CreateXor(C, ci32(Ctx, 0xffffffffu), Twine(NamePrefix) + ".not.c");
  switch (Gen() % 8) {
  case 0: {
    Value *KeepA = B.CreateAnd(A, Bv, Twine(NamePrefix) + ".mux.keep.a");
    Value *KeepC = B.CreateAnd(NotA, C, Twine(NamePrefix) + ".mux.keep.c");
    return B.CreateOr(KeepA, KeepC, Twine(NamePrefix) + ".mux");
  }
  case 1: {
    Value *AB = B.CreateAnd(A, Bv, Twine(NamePrefix) + ".maj.ab");
    Value *AC = B.CreateAnd(A, C, Twine(NamePrefix) + ".maj.ac");
    Value *BC = B.CreateAnd(Bv, C, Twine(NamePrefix) + ".maj.bc");
    return B.CreateOr(B.CreateOr(AB, AC, Twine(NamePrefix) + ".maj.or0"), BC,
                      Twine(NamePrefix) + ".majority");
  }
  case 2: {
    Value *Parity = B.CreateXor(B.CreateXor(A, Bv, Twine(NamePrefix) + ".pxor"),
                                C, Twine(NamePrefix) + ".parity");
    Value *AnyZero = B.CreateOr(B.CreateOr(NotA, NotB,
                                           Twine(NamePrefix) + ".anyzero0"),
                                NotC, Twine(NamePrefix) + ".anyzero");
    return B.CreateAnd(Parity, AnyZero, Twine(NamePrefix) + ".parity.masked");
  }
  case 3: {
    Value *T0 = B.CreateOr(A, Bv, Twine(NamePrefix) + ".cnf0");
    Value *T1 = B.CreateOr(NotA, C, Twine(NamePrefix) + ".cnf1");
    Value *T2 = B.CreateOr(NotB, NotC, Twine(NamePrefix) + ".cnf2");
    return B.CreateAnd(B.CreateAnd(T0, T1, Twine(NamePrefix) + ".cnf.and0"),
                       T2, Twine(NamePrefix) + ".cnf");
  }
  case 4: {
    Value *Mask = B.CreateXor(A, Bv, Twine(NamePrefix) + ".blend.mask");
    Value *Left = B.CreateAnd(C, Mask, Twine(NamePrefix) + ".blend.left");
    Value *Right =
        B.CreateAnd(Bv, B.CreateXor(Mask, ci32(Ctx, 0xffffffffu),
                                    Twine(NamePrefix) + ".blend.not"),
                    Twine(NamePrefix) + ".blend.right");
    return B.CreateOr(Left, Right, Twine(NamePrefix) + ".blend");
  }
  case 5: {
    Value *Cmp = B.CreateICmpULT(A, Bv, Twine(NamePrefix) + ".cmp");
    Value *Mask = buildPredicateMask(B, Cmp, Gen, Twine(NamePrefix) + ".pmask");
    return B.CreateXor(B.CreateAnd(A, Mask, Twine(NamePrefix) + ".mask.a"),
                       B.CreateOr(Bv, C, Twine(NamePrefix) + ".or.bc"),
                       Twine(NamePrefix) + ".pred.mix");
  }
  case 6: {
    Value *Any = B.CreateOr(B.CreateOr(A, Bv, Twine(NamePrefix) + ".any0"), C,
                            Twine(NamePrefix) + ".any");
    Value *All = B.CreateAnd(B.CreateAnd(A, Bv, Twine(NamePrefix) + ".all0"),
                             C, Twine(NamePrefix) + ".all");
    return B.CreateSub(Any, All, Twine(NamePrefix) + ".any.sub.all");
  }
  default: {
    Value *AB = B.CreateXor(A, Bv, Twine(NamePrefix) + ".ab");
    Value *BC = B.CreateOr(Bv, C, Twine(NamePrefix) + ".bc");
    Value *CA = B.CreateAnd(C, A, Twine(NamePrefix) + ".ca");
    return B.CreateAdd(B.CreateXor(AB, BC, Twine(NamePrefix) + ".mix0"), CA,
                       Twine(NamePrefix) + ".mix");
  }
  }
}

Value *emitRandomI64PairIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                              std::minstd_rand &Gen, StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *I64 = Type::getInt64Ty(Ctx);
  Value *A64 = B.CreateZExt(A, I64, Twine(NamePrefix) + ".a64");
  Value *B64 = B.CreateZExt(Bv, I64, Twine(NamePrefix) + ".b64");
  Value *C64 =
      ConstantInt::get(I64, static_cast<uint64_t>(randomInteresting64(Gen)));
  Value *Pair = B.CreateOr(B.CreateShl(A64, ConstantInt::get(I64, 32),
                                       Twine(NamePrefix) + ".pair.hi"),
                           B64, Twine(NamePrefix) + ".pair");
  auto Hi32 = [&](Value *V, const Twine &Name) {
    return B.CreateTrunc(B.CreateLShr(V, ConstantInt::get(I64, 32),
                                      Name + ".shr"),
                         I32, Name + ".i32");
  };
  auto Lo32 = [&](Value *V, const Twine &Name) {
    return B.CreateTrunc(V, I32, Name + ".i32");
  };

  switch (Gen() % 8) {
  case 0: {
    Value *Product = B.CreateMul(A64, B64, Twine(NamePrefix) + ".mul64");
    return B.CreateXor(Hi32(Product, Twine(NamePrefix) + ".mul.hi"),
                       Lo32(Product, Twine(NamePrefix) + ".mul.lo"),
                       Twine(NamePrefix) + ".mul.fold");
  }
  case 1: {
    Value *Sum = B.CreateAdd(B.CreateAdd(Pair, C64,
                                         Twine(NamePrefix) + ".pair.add.c"),
                             B64, Twine(NamePrefix) + ".pair.add.b");
    return B.CreateAdd(Hi32(Sum, Twine(NamePrefix) + ".sum.hi"),
                       Lo32(Sum, Twine(NamePrefix) + ".sum.lo"),
                       Twine(NamePrefix) + ".sum.fold");
  }
  case 2: {
    Value *Diff = B.CreateSub(Pair, B.CreateShl(B64, ConstantInt::get(I64, 1),
                                                Twine(NamePrefix) + ".b.shl1"),
                              Twine(NamePrefix) + ".pair.sub");
    return B.CreateXor(Hi32(Diff, Twine(NamePrefix) + ".diff.hi"), A,
                       Twine(NamePrefix) + ".diff.fold");
  }
  case 3: {
    unsigned Shift = 1 + (Gen() % 63);
    Value *Left = B.CreateShl(Pair, ConstantInt::get(I64, Shift),
                              Twine(NamePrefix) + ".rot.left");
    Value *Right = B.CreateLShr(Pair, ConstantInt::get(I64, 64 - Shift),
                                Twine(NamePrefix) + ".rot.right");
    Value *Rot = B.CreateOr(Left, Right, Twine(NamePrefix) + ".rot");
    return B.CreateSub(Hi32(Rot, Twine(NamePrefix) + ".rot.hi"),
                       Lo32(Rot, Twine(NamePrefix) + ".rot.lo"),
                       Twine(NamePrefix) + ".rot.fold");
  }
  case 4: {
    Value *Masked =
        B.CreateAnd(Pair, ConstantInt::get(I64, 0x00ff00ff00ff00ffULL),
                    Twine(NamePrefix) + ".mask64");
    Value *Spread = B.CreateMul(Masked, ConstantInt::get(I64, 0x10101ULL),
                                Twine(NamePrefix) + ".spread");
    return B.CreateXor(Hi32(Spread, Twine(NamePrefix) + ".spread.hi"),
                       Lo32(Spread, Twine(NamePrefix) + ".spread.lo"),
                       Twine(NamePrefix) + ".spread.fold");
  }
  case 5: {
    Value *Cmp = B.CreateICmpUGT(Pair, B.CreateXor(Pair, C64,
                                                   Twine(NamePrefix) + ".cmp.x"),
                                 Twine(NamePrefix) + ".cmp");
    Value *Sel = B.CreateSelect(Cmp, B.CreateAdd(Pair, A64,
                                                 Twine(NamePrefix) + ".sel.add"),
                                B.CreateSub(Pair, B64,
                                            Twine(NamePrefix) + ".sel.sub"),
                                Twine(NamePrefix) + ".sel");
    return B.CreateXor(Hi32(Sel, Twine(NamePrefix) + ".sel.hi"),
                       Lo32(Sel, Twine(NamePrefix) + ".sel.lo"),
                       Twine(NamePrefix) + ".sel.fold");
  }
  case 6: {
    Value *Lo = B.CreateAnd(Pair, ConstantInt::get(I64, 0xffff),
                            Twine(NamePrefix) + ".lo16");
    Value *Hi = B.CreateAnd(B.CreateLShr(Pair, ConstantInt::get(I64, 48),
                                         Twine(NamePrefix) + ".hi16.shr"),
                            ConstantInt::get(I64, 0xffff),
                            Twine(NamePrefix) + ".hi16");
    Value *Cross = B.CreateMul(B.CreateAdd(Lo, Hi,
                                           Twine(NamePrefix) + ".cross.add"),
                               ConstantInt::get(I64, 257),
                               Twine(NamePrefix) + ".cross.mul");
    return Lo32(Cross, Twine(NamePrefix) + ".cross");
  }
  default: {
    Value *SignedA = B.CreateSExt(B.CreateTrunc(A, Type::getInt16Ty(Ctx),
                                                Twine(NamePrefix) + ".a16"),
                                  I64, Twine(NamePrefix) + ".a16.s64");
    Value *SignedB = B.CreateSExt(B.CreateTrunc(Bv, Type::getInt16Ty(Ctx),
                                                Twine(NamePrefix) + ".b16"),
                                  I64, Twine(NamePrefix) + ".b16.s64");
    Value *Wide = B.CreateMul(SignedA, SignedB, Twine(NamePrefix) + ".smul");
    return B.CreateAdd(Lo32(Wide, Twine(NamePrefix) + ".smul.lo"),
                       Hi32(Wide, Twine(NamePrefix) + ".smul.hi"),
                       Twine(NamePrefix) + ".smul.fold");
  }
  }
}

Value *emitRandomBytePrefixIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                 std::minstd_rand &Gen,
                                 StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  SmallVector<Value *, 4> ABytes;
  SmallVector<Value *, 4> BBytes;
  for (unsigned I = 0; I != 4; ++I) {
    ABytes.push_back(extractByteAsI32(B, A, I, Twine(NamePrefix) + ".a.byte"));
    BBytes.push_back(extractByteAsI32(B, Bv, 3 - I,
                                      Twine(NamePrefix) + ".b.byte"));
  }

  switch (Gen() % 7) {
  case 0: {
    SmallVector<Value *, 4> Sums;
    Value *Carry = ci32(Ctx, Gen() & 1u);
    for (unsigned I = 0; I != 4; ++I) {
      Value *Sum = B.CreateAdd(B.CreateAdd(ABytes[I], BBytes[I],
                                           Twine(NamePrefix) + ".sum.ab"),
                               Carry, Twine(NamePrefix) + ".sum.carry");
      Sums.push_back(B.CreateAnd(Sum, ci32(Ctx, 0xff),
                                 Twine(NamePrefix) + ".sum.byte"));
      Carry = B.CreateLShr(Sum, ci32(Ctx, 8),
                           Twine(NamePrefix) + ".sum.carry.next");
    }
    return packFourBytesAsI32(B, Sums, (Gen() & 1u) != 0,
                              Twine(NamePrefix) + ".adc.pack");
  }
  case 1: {
    SmallVector<Value *, 4> Diffs;
    Value *Borrow = ci32(Ctx, Gen() & 1u);
    for (unsigned I = 0; I != 4; ++I) {
      Value *Subtrahend = B.CreateAdd(BBytes[I], Borrow,
                                      Twine(NamePrefix) + ".borrow.subtrahend");
      Value *Diff = B.CreateSub(ABytes[I], Subtrahend,
                                Twine(NamePrefix) + ".borrow.diff");
      Diffs.push_back(B.CreateAnd(Diff, ci32(Ctx, 0xff),
                                  Twine(NamePrefix) + ".borrow.byte"));
      Borrow = B.CreateZExt(B.CreateICmpULT(ABytes[I], Subtrahend,
                                            Twine(NamePrefix) + ".borrow.cmp"),
                            Type::getInt32Ty(Ctx),
                            Twine(NamePrefix) + ".borrow.next");
    }
    return packFourBytesAsI32(B, Diffs, false,
                              Twine(NamePrefix) + ".sbb.pack");
  }
  case 2: {
    SmallVector<Value *, 4> Prefix;
    Value *Acc = ci32(Ctx, 0);
    for (unsigned I = 0; I != 4; ++I) {
      Acc = B.CreateAdd(Acc, ABytes[I], Twine(NamePrefix) + ".prefix.add");
      Prefix.push_back(B.CreateAnd(Acc, ci32(Ctx, 0xff),
                                   Twine(NamePrefix) + ".prefix.byte"));
    }
    return packFourBytesAsI32(B, Prefix, true,
                              Twine(NamePrefix) + ".prefix.pack");
  }
  case 3: {
    SmallVector<Value *, 4> Sorted = ABytes;
    auto SortPair = [&](unsigned L, unsigned R) {
      Value *Lo = unsignedMinMaxSelect(B, Sorted[L], Sorted[R], false,
                                       Twine(NamePrefix) + ".sort.lo");
      Value *Hi = unsignedMinMaxSelect(B, Sorted[L], Sorted[R], true,
                                       Twine(NamePrefix) + ".sort.hi");
      Sorted[L] = Lo;
      Sorted[R] = Hi;
    };
    SortPair(0, 1);
    SortPair(2, 3);
    SortPair(0, 2);
    SortPair(1, 3);
    SortPair(1, 2);
    return packFourBytesAsI32(B, Sorted, (Gen() & 1u) != 0,
                              Twine(NamePrefix) + ".sort.pack");
  }
  case 4: {
    SmallVector<Value *, 4> Nibbles;
    for (unsigned I = 0; I != 4; ++I) {
      Value *Lo = B.CreateAnd(ABytes[I], ci32(Ctx, 0xf),
                              Twine(NamePrefix) + ".nib.lo");
      Value *Hi = B.CreateAnd(B.CreateLShr(BBytes[I], ci32(Ctx, 4),
                                           Twine(NamePrefix) + ".nib.shr"),
                              ci32(Ctx, 0xf), Twine(NamePrefix) + ".nib.hi");
      Nibbles.push_back(B.CreateOr(Lo, B.CreateShl(Hi, ci32(Ctx, 4),
                                                   Twine(NamePrefix) + ".nib.shl"),
                                   Twine(NamePrefix) + ".nib.byte"));
    }
    return packFourBytesAsI32(B, Nibbles, false,
                              Twine(NamePrefix) + ".nib.pack");
  }
  case 5: {
    SmallVector<Value *, 4> Selected;
    for (unsigned I = 0; I != 4; ++I) {
      Value *Cmp = B.CreateICmpULT(ABytes[I], BBytes[I],
                                   Twine(NamePrefix) + ".sel.cmp");
      Value *Mask = buildPredicateMask(B, Cmp, Gen,
                                       Twine(NamePrefix) + ".sel.mask");
      Selected.push_back(B.CreateAnd(
          B.CreateOr(B.CreateAnd(ABytes[I], Mask,
                                 Twine(NamePrefix) + ".sel.a"),
                     B.CreateAnd(BBytes[I],
                                 B.CreateXor(Mask, ci32(Ctx, 0xffffffffu),
                                             Twine(NamePrefix) + ".sel.not"),
                                 Twine(NamePrefix) + ".sel.b"),
                     Twine(NamePrefix) + ".sel.or"),
          ci32(Ctx, 0xff), Twine(NamePrefix) + ".sel.byte"));
    }
    return packFourBytesAsI32(B, Selected, true,
                              Twine(NamePrefix) + ".sel.pack");
  }
  default: {
    SmallVector<Value *, 4> Mixed;
    for (unsigned I = 0; I != 4; ++I) {
      Value *Rot = B.CreateOr(
          B.CreateShl(ABytes[I], ci32(Ctx, (I + 1) & 7u),
                      Twine(NamePrefix) + ".byte.rot.l"),
          B.CreateLShr(ABytes[I], ci32(Ctx, 8 - ((I + 1) & 7u)),
                       Twine(NamePrefix) + ".byte.rot.r"),
          Twine(NamePrefix) + ".byte.rot");
      Mixed.push_back(B.CreateAnd(B.CreateXor(Rot, BBytes[I],
                                              Twine(NamePrefix) + ".byte.xor"),
                                  ci32(Ctx, 0xff),
                                  Twine(NamePrefix) + ".byte.mix"));
    }
    return packFourBytesAsI32(B, Mixed, (Gen() & 1u) != 0,
                              Twine(NamePrefix) + ".mix.pack");
  }
  }
}

Value *emitRandomOverflowChainIdiom(IRBuilder<NoFolder> &B, Module &M,
                                    Value *A, Value *Bv,
                                    std::minstd_rand &Gen,
                                    StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *C = interestingI32(Ctx, Gen);

  auto CallOverflow = [&](Intrinsic::ID ID, Value *X, Value *Y,
                          const Twine &Name) {
    FunctionCallee Fn = Intrinsic::getOrInsertDeclaration(&M, ID, {I32});
    Value *Pair = B.CreateCall(Fn, {X, Y}, Name + ".call");
    Value *ResultValue = B.CreateExtractValue(Pair, {0}, Name + ".value");
    Value *OverflowBit = B.CreateExtractValue(Pair, {1}, Name + ".overflow");
    return std::pair<Value *, Value *>(ResultValue, OverflowBit);
  };

  auto AddPair = CallOverflow(Intrinsic::uadd_with_overflow, A, Bv,
                              Twine(NamePrefix) + ".uadd0");
  Value *Add0 = AddPair.first;
  Value *AddOv0 = AddPair.second;
  Value *Carry0 = B.CreateZExt(AddOv0, I32, Twine(NamePrefix) + ".carry0");
  auto SubPair =
      CallOverflow(Intrinsic::usub_with_overflow, Add0,
                   B.CreateAdd(C, Carry0, Twine(NamePrefix) + ".sub.rhs"),
                   Twine(NamePrefix) + ".usub0");
  Value *Sub0 = SubPair.first;
  Value *SubOv0 = SubPair.second;
  Value *Borrow0 = B.CreateZExt(SubOv0, I32, Twine(NamePrefix) + ".borrow0");

  switch (Gen() % 6) {
  case 0:
    return B.CreateAdd(Sub0, Borrow0, Twine(NamePrefix) + ".sub.borrow.add");
  case 1: {
    auto MulPair =
        CallOverflow(Intrinsic::umul_with_overflow, Sub0,
                     B.CreateOr(Bv, ci32(Ctx, 1), Twine(NamePrefix) + ".mul.nz"),
                     Twine(NamePrefix) + ".umul0");
    Value *Mul0 = MulPair.first;
    Value *MulOv0 = MulPair.second;
    Value *MulOvI32 = B.CreateZExt(MulOv0, I32, Twine(NamePrefix) + ".mul.ov");
    return B.CreateXor(Mul0, MulOvI32, Twine(NamePrefix) + ".mul.mix");
  }
  case 2: {
    auto SAddPair =
        CallOverflow(Intrinsic::sadd_with_overflow, Sub0, C,
                     Twine(NamePrefix) + ".sadd0");
    Value *SAdd = SAddPair.first;
    Value *SAddOv = SAddPair.second;
    Value *Mask = buildPredicateMask(B, SAddOv, Gen,
                                     Twine(NamePrefix) + ".sadd.mask");
    return B.CreateOr(B.CreateAnd(SAdd, Mask, Twine(NamePrefix) + ".sadd.keep"),
                      B.CreateAnd(A,
                                  B.CreateXor(Mask, ci32(Ctx, 0xffffffffu),
                                              Twine(NamePrefix) + ".sadd.not"),
                                  Twine(NamePrefix) + ".sadd.fallback"),
                      Twine(NamePrefix) + ".sadd.select");
  }
  case 3: {
    auto SSubPair =
        CallOverflow(Intrinsic::ssub_with_overflow, A, Sub0,
                     Twine(NamePrefix) + ".ssub0");
    Value *SSub = SSubPair.first;
    Value *SSubOv = SSubPair.second;
    Value *SSubOvI32 = B.CreateZExt(SSubOv, I32, Twine(NamePrefix) + ".ssub.ov");
    return B.CreateSub(SSub, SSubOvI32, Twine(NamePrefix) + ".ssub.mix");
  }
  case 4: {
    Value *Both = B.CreateAnd(AddOv0, SubOv0, Twine(NamePrefix) + ".both.ov");
    return B.CreateSelect(Both, B.CreateXor(Sub0, C,
                                            Twine(NamePrefix) + ".both.xor"),
                          B.CreateAdd(Sub0, Carry0,
                                      Twine(NamePrefix) + ".both.add"),
                          Twine(NamePrefix) + ".both.select");
  }
  default: {
    Value *Any = B.CreateOr(AddOv0, SubOv0, Twine(NamePrefix) + ".any.ov");
    Value *AnyI32 = B.CreateZExt(Any, I32, Twine(NamePrefix) + ".any.i32");
    return B.CreateXor(B.CreateAdd(Add0, Sub0, Twine(NamePrefix) + ".sum"),
                       AnyI32, Twine(NamePrefix) + ".any.mix");
  }
  }
}

Value *emitRandomSelectLookupIdiom(IRBuilder<NoFolder> &B, Value *A, Value *Bv,
                                   std::minstd_rand &Gen,
                                   StringRef NamePrefix) {
  LLVMContext &Ctx = A->getContext();
  Value *IndexSeed = (Gen() & 1u) ? A : Bv;
  Value *Index = B.CreateAnd(IndexSeed, ci32(Ctx, 7),
                             Twine(NamePrefix) + ".index");
  Value *Result = interestingI32(Ctx, Gen);
  for (unsigned I = 0; I != 8; ++I) {
    Value *Base = (I & 1u) ? A : Bv;
    Value *Other = (I & 1u) ? Bv : A;
    Value *Entry = nullptr;
    switch ((Gen() + I) % 5) {
    case 0:
      Entry = B.CreateAdd(Base, ci32(Ctx, I * 17 + 1),
                          Twine(NamePrefix) + ".entry.add");
      break;
    case 1:
      Entry = B.CreateXor(Base, B.CreateShl(Other, ci32(Ctx, I & 7u),
                                            Twine(NamePrefix) + ".entry.shl"),
                          Twine(NamePrefix) + ".entry.xor");
      break;
    case 2:
      Entry = B.CreateOr(B.CreateAnd(Base, ci32(Ctx, 0xffu << ((I & 3u) * 8)),
                                     Twine(NamePrefix) + ".entry.mask"),
                         Other, Twine(NamePrefix) + ".entry.or");
      break;
    case 3:
      Entry = B.CreateSub(B.CreateLShr(Base, ci32(Ctx, I & 15u),
                                       Twine(NamePrefix) + ".entry.shr"),
                          Other, Twine(NamePrefix) + ".entry.sub");
      break;
    default: {
      Value *Cmp = B.CreateICmpULT(Base, Other, Twine(NamePrefix) + ".entry.cmp");
      Entry = B.CreateSelect(Cmp, Base, Other, Twine(NamePrefix) + ".entry.sel");
      break;
    }
    }
    Value *IsEntry = B.CreateICmpEQ(Index, ci32(Ctx, I),
                                    Twine(NamePrefix) + ".entry.is");
    Result = B.CreateSelect(IsEntry, Entry, Result,
                            Twine(NamePrefix) + ".entry.pick");
  }
  return Result;
}

Value *emitRandomNibbleReduceIdiom(IRBuilder<NoFolder> &B, Module &M, Value *A,
                                   Value *Bv, std::minstd_rand &Gen,
                                   StringRef NamePrefix) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  FunctionCallee Ctpop =
      Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctpop, {I32});
  FunctionCallee Cttz =
      Intrinsic::getOrInsertDeclaration(&M, Intrinsic::cttz, {I32});
  SmallVector<Value *, 8> Nibbles;
  for (unsigned I = 0; I != 8; ++I) {
    Value *Src = (I & 1u) ? A : Bv;
    Value *Shifted = B.CreateLShr(Src, ci32(Ctx, I * 4),
                                  Twine(NamePrefix) + ".nib.shr");
    Nibbles.push_back(B.CreateAnd(Shifted, ci32(Ctx, 0xf),
                                  Twine(NamePrefix) + ".nib"));
  }

  switch (Gen() % 6) {
  case 0: {
    Value *Packed = ci32(Ctx, 0);
    for (unsigned I = 0; I != 8; ++I) {
      Value *Pop = B.CreateCall(Ctpop, {Nibbles[I]},
                                Twine(NamePrefix) + ".pop");
      Value *Nib = B.CreateAnd(Pop, ci32(Ctx, 0xf),
                               Twine(NamePrefix) + ".pop.nib");
      Packed = B.CreateOr(Packed, B.CreateShl(Nib, ci32(Ctx, I * 4),
                                              Twine(NamePrefix) + ".pop.shl"),
                          Twine(NamePrefix) + ".pop.pack");
    }
    return Packed;
  }
  case 1: {
    Value *Count = ci32(Ctx, 0);
    for (Value *Nib : Nibbles) {
      Value *NonZero =
          B.CreateZExt(B.CreateICmpNE(Nib, ci32(Ctx, 0),
                                      Twine(NamePrefix) + ".nz"),
                       I32, Twine(NamePrefix) + ".nz.i32");
      Count = B.CreateAdd(Count, NonZero, Twine(NamePrefix) + ".nz.count");
    }
    return B.CreateXor(Count, A, Twine(NamePrefix) + ".nz.mix");
  }
  case 2: {
    Value *First = ci32(Ctx, 8);
    for (int I = 7; I >= 0; --I) {
      Value *NonZero = B.CreateICmpNE(Nibbles[I], ci32(Ctx, 0),
                                      Twine(NamePrefix) + ".first.nz");
      First = B.CreateSelect(NonZero, ci32(Ctx, I), First,
                             Twine(NamePrefix) + ".first.select");
    }
    return B.CreateAdd(First, Bv, Twine(NamePrefix) + ".first.add");
  }
  case 3: {
    Value *Parity = ci32(Ctx, 0);
    for (unsigned I = 0; I != 8; ++I) {
      Value *Pop = B.CreateCall(Ctpop, {Nibbles[I]},
                                Twine(NamePrefix) + ".parity.pop");
      Value *Bit = B.CreateAnd(Pop, ci32(Ctx, 1),
                               Twine(NamePrefix) + ".parity.bit");
      Parity = B.CreateOr(Parity, B.CreateShl(Bit, ci32(Ctx, I),
                                              Twine(NamePrefix) + ".parity.shl"),
                          Twine(NamePrefix) + ".parity.pack");
    }
    return B.CreateXor(Parity, Bv, Twine(NamePrefix) + ".parity.mix");
  }
  case 4: {
    Value *Packed = ci32(Ctx, 0);
    for (unsigned I = 0; I != 8; ++I) {
      Value *Tz = B.CreateCall(Cttz, {Nibbles[I], ConstantInt::getFalse(Ctx)},
                               Twine(NamePrefix) + ".cttz");
      Value *Nib = B.CreateAnd(Tz, ci32(Ctx, 0xf),
                               Twine(NamePrefix) + ".cttz.nib");
      Packed = B.CreateOr(Packed, B.CreateShl(Nib, ci32(Ctx, I * 4),
                                              Twine(NamePrefix) + ".cttz.shl"),
                          Twine(NamePrefix) + ".cttz.pack");
    }
    return Packed;
  }
  default: {
    Value *Accum = ci32(Ctx, Gen() & 0xffu);
    for (unsigned I = 0; I != 8; ++I) {
      Value *Scaled = B.CreateMul(Nibbles[I], ci32(Ctx, I + 1),
                                  Twine(NamePrefix) + ".scaled");
      Accum = (I & 1u) ? B.CreateXor(Accum, Scaled,
                                     Twine(NamePrefix) + ".acc.xor")
                       : B.CreateAdd(Accum, Scaled,
                                     Twine(NamePrefix) + ".acc.add");
    }
    return Accum;
  }
  }
}

Value *emitRandomIRInstruction(IRBuilder<NoFolder> &B, Module &M,
                               Instruction *InsertPt, Value *Current,
                               std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *A = Current;
  Value *Bv = chooseI32Value(InsertPt, Gen);
  switch (Gen() % 266) {
  case 0:
    return B.CreateAdd(A, Bv, "fuzz.add");
  case 1:
    return B.CreateSub(A, Bv, "fuzz.sub");
  case 2:
    return B.CreateMul(A, Bv, "fuzz.mul");
  case 3:
    return B.CreateXor(A, Bv, "fuzz.xor");
  case 4:
    return B.CreateAnd(A, Bv, "fuzz.and");
  case 5:
    return B.CreateOr(A, Bv, "fuzz.or");
  case 6:
    return B.CreateShl(A, ci32(Ctx, Gen() & 31u), "fuzz.shl");
  case 7:
    return B.CreateLShr(A, ci32(Ctx, Gen() & 31u), "fuzz.lshr");
  case 8:
    return B.CreateAShr(A, ci32(Ctx, Gen() & 31u), "fuzz.ashr");
  case 9:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctpop, {I32}), {A},
        "fuzz.ctpop");
  case 10:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bitreverse, {I32}),
        {A}, "fuzz.bitreverse");
  case 11:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bswap, {I32}), {A},
        "fuzz.bswap");
  case 12:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctlz, {I32}),
        {A, ConstantInt::getFalse(Ctx)}, "fuzz.ctlz");
  case 13:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::cttz, {I32}),
        {A, ConstantInt::getFalse(Ctx)}, "fuzz.cttz");
  case 14:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umin, {I32}), {A, Bv},
        "fuzz.umin");
  case 15:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umax, {I32}), {A, Bv},
        "fuzz.umax");
  case 16:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smin, {I32}), {A, Bv},
        "fuzz.smin");
  case 17:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smax, {I32}), {A, Bv},
        "fuzz.smax");
  case 18:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::uadd_sat, {I32}),
        {A, Bv}, "fuzz.uadd_sat");
  case 19:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::usub_sat, {I32}),
        {A, Bv}, "fuzz.usub_sat");
  case 20:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sadd_sat, {I32}),
        {A, Bv}, "fuzz.sadd_sat");
  case 21:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ssub_sat, {I32}),
        {A, Bv}, "fuzz.ssub_sat");
  case 22: {
    Value *Cmp = B.CreateICmp(randomICmpPredicate(Gen), A, Bv, "fuzz.cmp");
    return B.CreateSelect(Cmp, Bv, A, "fuzz.select");
  }
  case 23:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I32}),
        {A, Bv, ci32(Ctx, Gen() & 31u)}, "fuzz.fshl");
  case 24:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I32}),
        {A, Bv, ci32(Ctx, Gen() & 31u)}, "fuzz.fshr");
  case 25:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::abs, {I32}),
        {A, ConstantInt::getFalse(Ctx)}, "fuzz.abs");
  case 26: {
    Value *Den = B.CreateOr(Bv, ci32(Ctx, 1), "fuzz.nz");
    return B.CreateUDiv(A, Den, "fuzz.udiv");
  }
  case 27: {
    Value *Den = B.CreateOr(Bv, ci32(Ctx, 1), "fuzz.nz");
    return B.CreateURem(A, Den, "fuzz.urem");
  }
  case 28:
    return B.CreateZExt(B.CreateTrunc(A, I8, "fuzz.trunc.i8"), I32,
                        "fuzz.zext.i8");
  case 29:
    return B.CreateSExt(B.CreateTrunc(A, I8, "fuzz.trunc.i8"), I32,
                        "fuzz.sext.i8");
  case 30:
    return B.CreateZExt(B.CreateTrunc(A, I16, "fuzz.trunc.i16"), I32,
                        "fuzz.zext.i16");
  case 31:
    return B.CreateSExt(B.CreateTrunc(A, I16, "fuzz.trunc.i16"), I32,
                        "fuzz.sext.i16");
  case 32:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I32}),
        {A, Bv, chooseI32Value(InsertPt, Gen)}, "fuzz.fshl.dyn");
  case 33:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I32}),
        {A, Bv, chooseI32Value(InsertPt, Gen)}, "fuzz.fshr.dyn");
  case 34:
  case 35:
  case 36:
  case 37:
  case 38:
  case 39:
    return emitRandomI64Instruction(B, M, A, Bv, Gen);
  case 40:
  case 41:
  case 42:
  case 43:
  case 44:
  case 45:
    return emitRandomBoolI32Instruction(B, A, Bv, Gen, "fuzz.bool");
  case 46:
  case 47:
  case 48:
  case 49:
  case 50:
  case 51:
    return emitRandomFiniteFPInstruction(B, A, Bv, Gen, "fuzz.fp");
  case 52:
    return emitSafeSignedDivRemInstruction(B, A, Bv, false, "fuzz.sdiv");
  case 53:
    return emitSafeSignedDivRemInstruction(B, A, Bv, true, "fuzz.srem");
  case 54:
  case 55:
  case 56:
  case 57:
    return emitRandomFiniteSignedFPInstruction(B, A, Bv, Gen,
                                               "fuzz.fp.signed");
  case 58:
  case 59:
  case 60:
  case 61:
    return emitSafeInputLoadInstruction(B, M, A, Bv, Gen, "fuzz.load");
  case 62:
  case 63:
  case 64:
  case 65:
  case 66:
  case 67:
    return emitRandomFiniteHalfFPInstruction(B, A, Bv, Gen, "fuzz.fp16");
  case 68:
  case 69:
  case 70:
  case 71:
  case 72:
  case 73:
    return emitRandomOverflowInstruction(B, M, A, Bv, Gen, "fuzz.overflow");
  case 74:
  case 75:
  case 76:
  case 77:
  case 78:
  case 79:
    return emitRandomAMDGPUIntrinsicInstruction(B, M, A, Bv, Gen,
                                                "fuzz.amdgcn");
  case 80:
  case 81:
  case 82:
  case 83:
  case 84:
  case 85:
    return emitRandomNarrowScalarInstruction(B, M, A, Bv, Gen, "fuzz.narrow");
  case 86:
  case 87:
  case 88:
  case 89:
  case 90:
  case 91:
    return emitRandomUnsignedSelectIdiom(B, A, Bv, Gen, "fuzz.select.idiom");
  case 92:
  case 93:
  case 94:
  case 95:
  case 96:
  case 97:
    return emitRandomManualFunnelShiftIdiom(B, A, Bv, Gen,
                                            "fuzz.funnel.idiom");
  case 98:
  case 99:
  case 100:
  case 101:
  case 102:
  case 103:
    return emitRandomSignedOverflowSelectIdiom(B, A, Bv, Gen,
                                               "fuzz.soverflow.idiom");
  case 104:
  case 105:
  case 106:
  case 107:
  case 108:
  case 109:
    return emitRandomPredicateMaskIdiom(B, A, Bv, Gen, "fuzz.predmask.idiom");
  case 110:
  case 111:
  case 112:
  case 113:
  case 114:
  case 115:
    return emitRandomBitfieldIdiom(B, A, Bv, Gen, "fuzz.bitfield.idiom");
  case 116:
  case 117:
  case 118:
  case 119:
  case 120:
  case 121:
    return emitRandomWideningMulIdiom(B, A, Bv, Gen, "fuzz.widemul.idiom");
  case 122:
  case 123:
  case 124:
  case 125:
  case 126:
  case 127:
  case 128:
  case 129:
    return emitRandomPackUnpackIdiom(B, A, Bv, Gen,
                                     "fuzz.packunpack.idiom");
  case 130:
  case 131:
  case 132:
  case 133:
  case 134:
  case 135:
  case 136:
  case 137:
    return emitRandomBitCountIdiom(B, M, A, Bv, Gen,
                                   "fuzz.bitcount.idiom");
  case 138:
  case 139:
  case 140:
  case 141:
  case 142:
  case 143:
  case 144:
  case 145:
    return emitRandomAverageDiffIdiom(B, A, Bv, Gen,
                                      "fuzz.avgdiff.idiom");
  case 146:
  case 147:
  case 148:
  case 149:
  case 150:
  case 151:
  case 152:
  case 153:
    return emitRandomClampPackIdiom(B, A, Bv, Gen,
                                    "fuzz.clamppack.idiom");
  case 154:
  case 155:
  case 156:
  case 157:
  case 158:
  case 159:
  case 160:
  case 161:
    return emitRandomVectorReductionIdiom(B, A, Bv, Gen,
                                          "fuzz.vecreduce.idiom");
  case 162:
  case 163:
  case 164:
  case 165:
  case 166:
  case 167:
  case 168:
  case 169:
    return emitRandomByteDotChainIdiom(B, A, Bv, Gen,
                                       "fuzz.bytedot.idiom");
  case 170:
  case 171:
  case 172:
  case 173:
  case 174:
  case 175:
  case 176:
  case 177:
    return emitRandomMaskedShiftInstruction(B, A, Bv, Gen,
                                            "fuzz.maskshift");
  case 178:
  case 179:
  case 180:
  case 181:
  case 182:
  case 183:
  case 184:
  case 185:
    return emitRandomCarryChainIdiom(B, A, Bv, Gen, "fuzz.carry.idiom");
  case 186:
  case 187:
  case 188:
  case 189:
  case 190:
  case 191:
  case 192:
  case 193:
    return emitRandomDynamicByteIdiom(B, A, Bv, Gen, "fuzz.dynbyte.idiom");
  case 194:
  case 195:
  case 196:
  case 197:
  case 198:
  case 199:
  case 200:
  case 201:
    return emitRandomCompareRankIdiom(B, A, Bv, Gen, "fuzz.rank.idiom");
  case 202:
  case 203:
  case 204:
  case 205:
  case 206:
  case 207:
  case 208:
  case 209:
    return emitRandomTernaryBitIdiom(B, A, Bv, Gen, "fuzz.ternary.idiom");
  case 210:
  case 211:
  case 212:
  case 213:
  case 214:
  case 215:
  case 216:
  case 217:
    return emitRandomI64PairIdiom(B, A, Bv, Gen, "fuzz.i64pair.idiom");
  case 218:
  case 219:
  case 220:
  case 221:
  case 222:
  case 223:
  case 224:
  case 225:
    return emitRandomBytePrefixIdiom(B, A, Bv, Gen, "fuzz.byteprefix.idiom");
  case 226:
  case 227:
  case 228:
  case 229:
  case 230:
  case 231:
  case 232:
  case 233:
    return emitRandomOverflowChainIdiom(B, M, A, Bv, Gen,
                                        "fuzz.ovchain.idiom");
  case 234:
  case 235:
  case 236:
  case 237:
  case 238:
  case 239:
  case 240:
  case 241:
    return emitRandomSelectLookupIdiom(B, A, Bv, Gen,
                                       "fuzz.lookup.idiom");
  case 242:
  case 243:
  case 244:
  case 245:
  case 246:
  case 247:
  case 248:
  case 249:
    return emitRandomNibbleReduceIdiom(B, M, A, Bv, Gen,
                                       "fuzz.nibble.idiom");
  default:
    switch (Gen() % 5) {
    case 0:
      return emitRandomNarrowVectorInstruction(B, M, A, Bv, Gen);
    case 1:
      return emitRandomVectorFPInstruction(B, M, A, Bv, Gen);
    case 2:
      return emitRandomVectorHalfFPInstruction(B, M, A, Bv, Gen);
    default:
      break;
    }
    return emitRandomVectorInstruction(B, M, A, Bv, Gen);
  }
}

void mutateIRAddInstruction(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F)
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;
  IRBuilder<NoFolder> B(Store);
  Value *Current = Store->getValueOperand();
  Value *Next = emitRandomIRInstruction(B, M, Store, Current, Gen);
  Store->setOperand(0, Next);
}

Value *emitRandomCFGArmInstruction(IRBuilder<NoFolder> &B, Module &M, Value *A,
                                   Value *Bv, std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  switch (Gen() % 250) {
  case 0:
    return B.CreateAdd(A, Bv, "fuzz.cfg.add");
  case 1:
    return B.CreateSub(A, Bv, "fuzz.cfg.sub");
  case 2:
    return B.CreateMul(A, Bv, "fuzz.cfg.mul");
  case 3:
    return B.CreateXor(A, Bv, "fuzz.cfg.xor");
  case 4:
    return B.CreateAnd(A, Bv, "fuzz.cfg.and");
  case 5:
    return B.CreateOr(A, Bv, "fuzz.cfg.or");
  case 6:
    return B.CreateShl(A, ci32(Ctx, Gen() & 31u), "fuzz.cfg.shl");
  case 7:
    return B.CreateLShr(A, ci32(Ctx, Gen() & 31u), "fuzz.cfg.lshr");
  case 8:
    return B.CreateAShr(A, ci32(Ctx, Gen() & 31u), "fuzz.cfg.ashr");
  case 9:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctpop, {I32}), {A},
        "fuzz.cfg.ctpop");
  case 10:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bitreverse, {I32}),
        {A}, "fuzz.cfg.bitreverse");
  case 11:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::bswap, {I32}), {A},
        "fuzz.cfg.bswap");
  case 12:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ctlz, {I32}),
        {A, ConstantInt::getFalse(Ctx)}, "fuzz.cfg.ctlz");
  case 13:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::cttz, {I32}),
        {A, ConstantInt::getFalse(Ctx)}, "fuzz.cfg.cttz");
  case 14:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umin, {I32}), {A, Bv},
        "fuzz.cfg.umin");
  case 15:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::umax, {I32}), {A, Bv},
        "fuzz.cfg.umax");
  case 16:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smin, {I32}), {A, Bv},
        "fuzz.cfg.smin");
  case 17:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::smax, {I32}), {A, Bv},
        "fuzz.cfg.smax");
  case 18: {
    Value *Cmp = B.CreateICmp(randomICmpPredicate(Gen), A, Bv, "fuzz.cfg.cmp");
    return B.CreateSelect(Cmp, A, Bv, "fuzz.cfg.select");
  }
  case 19:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::abs, {I32}),
        {A, ConstantInt::getFalse(Ctx)}, "fuzz.cfg.abs");
  case 20:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::uadd_sat, {I32}),
        {A, Bv}, "fuzz.cfg.uadd_sat");
  case 21:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::usub_sat, {I32}),
        {A, Bv}, "fuzz.cfg.usub_sat");
  case 22:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::sadd_sat, {I32}),
        {A, Bv}, "fuzz.cfg.sadd_sat");
  case 23:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::ssub_sat, {I32}),
        {A, Bv}, "fuzz.cfg.ssub_sat");
  case 24:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I32}),
        {A, Bv, ci32(Ctx, Gen() & 31u)}, "fuzz.cfg.fshl");
  case 25:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I32}),
        {A, Bv, ci32(Ctx, Gen() & 31u)}, "fuzz.cfg.fshr");
  case 26:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshl, {I32}),
        {A, Bv, B.CreateAnd(Bv, ci32(Ctx, 31), "fuzz.cfg.shift")},
        "fuzz.cfg.fshl.dyn");
  case 27:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I32}),
        {A, Bv, B.CreateAnd(A, ci32(Ctx, 31), "fuzz.cfg.shift")},
        "fuzz.cfg.fshr.dyn");
  case 28:
    return B.CreateZExt(B.CreateTrunc(A, I8, "fuzz.cfg.trunc.i8"), I32,
                        "fuzz.cfg.zext.i8");
  case 29:
    return B.CreateSExt(B.CreateTrunc(A, I8, "fuzz.cfg.trunc.i8"), I32,
                        "fuzz.cfg.sext.i8");
  case 30:
    return B.CreateZExt(B.CreateTrunc(A, I16, "fuzz.cfg.trunc.i16"), I32,
                        "fuzz.cfg.zext.i16");
  case 31:
    return B.CreateSExt(B.CreateTrunc(A, I16, "fuzz.cfg.trunc.i16"), I32,
                        "fuzz.cfg.sext.i16");
  case 32:
  case 33:
  case 34:
  case 35:
  case 36:
  case 37:
    return emitRandomBoolI32Instruction(B, A, Bv, Gen, "fuzz.cfg.bool");
  case 38:
  case 39:
  case 40:
  case 41:
  case 42:
  case 43:
    return emitRandomFiniteFPInstruction(B, A, Bv, Gen, "fuzz.cfg.fp");
  case 44:
    return emitSafeSignedDivRemInstruction(B, A, Bv, false, "fuzz.cfg.sdiv");
  case 45:
    return emitSafeSignedDivRemInstruction(B, A, Bv, true, "fuzz.cfg.srem");
  case 46:
  case 47:
  case 48:
  case 49:
    return emitRandomFiniteSignedFPInstruction(B, A, Bv, Gen,
                                               "fuzz.cfg.fp.signed");
  case 50:
  case 51:
  case 52:
  case 53:
    return emitSafeInputLoadInstruction(B, M, A, Bv, Gen, "fuzz.load");
  case 54:
  case 55:
  case 56:
  case 57:
  case 58:
  case 59:
    return emitRandomFiniteHalfFPInstruction(B, A, Bv, Gen, "fuzz.cfg.fp16");
  case 60:
  case 61:
  case 62:
  case 63:
  case 64:
  case 65:
    return emitRandomOverflowInstruction(B, M, A, Bv, Gen,
                                         "fuzz.cfg.overflow");
  case 66:
  case 67:
  case 68:
  case 69:
  case 70:
  case 71:
    return emitRandomAMDGPUIntrinsicInstruction(B, M, A, Bv, Gen,
                                                "fuzz.cfg.amdgcn");
  case 72:
  case 73:
  case 74:
  case 75:
  case 76:
  case 77:
    return emitRandomNarrowScalarInstruction(B, M, A, Bv, Gen,
                                             "fuzz.cfg.narrow");
  case 78:
  case 79:
  case 80:
  case 81:
  case 82:
  case 83:
    return emitRandomUnsignedSelectIdiom(B, A, Bv, Gen,
                                         "fuzz.cfg.select.idiom");
  case 84:
  case 85:
  case 86:
  case 87:
  case 88:
  case 89:
    return emitRandomManualFunnelShiftIdiom(B, A, Bv, Gen,
                                            "fuzz.cfg.funnel.idiom");
  case 90:
  case 91:
  case 92:
  case 93:
  case 94:
  case 95:
    return emitRandomSignedOverflowSelectIdiom(B, A, Bv, Gen,
                                               "fuzz.cfg.soverflow.idiom");
  case 96:
  case 97:
  case 98:
  case 99:
  case 100:
  case 101:
    return emitRandomPredicateMaskIdiom(B, A, Bv, Gen,
                                        "fuzz.cfg.predmask.idiom");
  case 102:
  case 103:
  case 104:
  case 105:
  case 106:
  case 107:
    return emitRandomBitfieldIdiom(B, A, Bv, Gen,
                                   "fuzz.cfg.bitfield.idiom");
  case 108:
  case 109:
  case 110:
  case 111:
  case 112:
  case 113:
    return emitRandomWideningMulIdiom(B, A, Bv, Gen,
                                      "fuzz.cfg.widemul.idiom");
  case 114:
  case 115:
  case 116:
  case 117:
  case 118:
  case 119:
  case 120:
  case 121:
    return emitRandomPackUnpackIdiom(B, A, Bv, Gen,
                                     "fuzz.cfg.packunpack.idiom");
  case 122:
  case 123:
  case 124:
  case 125:
  case 126:
  case 127:
  case 128:
  case 129:
    return emitRandomBitCountIdiom(B, M, A, Bv, Gen,
                                   "fuzz.cfg.bitcount.idiom");
  case 130:
  case 131:
  case 132:
  case 133:
  case 134:
  case 135:
  case 136:
  case 137:
    return emitRandomAverageDiffIdiom(B, A, Bv, Gen,
                                      "fuzz.cfg.avgdiff.idiom");
  case 138:
  case 139:
  case 140:
  case 141:
  case 142:
  case 143:
  case 144:
  case 145:
    return emitRandomClampPackIdiom(B, A, Bv, Gen,
                                    "fuzz.cfg.clamppack.idiom");
  case 146:
  case 147:
  case 148:
  case 149:
  case 150:
  case 151:
  case 152:
  case 153:
    return emitRandomVectorReductionIdiom(B, A, Bv, Gen,
                                          "fuzz.cfg.vecreduce.idiom");
  case 154:
  case 155:
  case 156:
  case 157:
  case 158:
  case 159:
  case 160:
  case 161:
    return emitRandomByteDotChainIdiom(B, A, Bv, Gen,
                                       "fuzz.cfg.bytedot.idiom");
  case 162:
  case 163:
  case 164:
  case 165:
  case 166:
  case 167:
  case 168:
  case 169:
    return emitRandomMaskedShiftInstruction(B, A, Bv, Gen,
                                            "fuzz.cfg.maskshift");
  case 170:
  case 171:
  case 172:
  case 173:
  case 174:
  case 175:
  case 176:
  case 177:
    return emitRandomCarryChainIdiom(B, A, Bv, Gen, "fuzz.cfg.carry.idiom");
  case 178:
  case 179:
  case 180:
  case 181:
  case 182:
  case 183:
  case 184:
  case 185:
    return emitRandomDynamicByteIdiom(B, A, Bv, Gen, "fuzz.cfg.dynbyte.idiom");
  case 186:
  case 187:
  case 188:
  case 189:
  case 190:
  case 191:
  case 192:
  case 193:
    return emitRandomCompareRankIdiom(B, A, Bv, Gen, "fuzz.cfg.rank.idiom");
  case 194:
  case 195:
  case 196:
  case 197:
  case 198:
  case 199:
  case 200:
  case 201:
    return emitRandomTernaryBitIdiom(B, A, Bv, Gen,
                                     "fuzz.cfg.ternary.idiom");
  case 202:
  case 203:
  case 204:
  case 205:
  case 206:
  case 207:
  case 208:
  case 209:
    return emitRandomI64PairIdiom(B, A, Bv, Gen,
                                  "fuzz.cfg.i64pair.idiom");
  case 210:
  case 211:
  case 212:
  case 213:
  case 214:
  case 215:
  case 216:
  case 217:
    return emitRandomBytePrefixIdiom(B, A, Bv, Gen,
                                     "fuzz.cfg.byteprefix.idiom");
  case 218:
  case 219:
  case 220:
  case 221:
  case 222:
  case 223:
  case 224:
  case 225:
    return emitRandomOverflowChainIdiom(B, M, A, Bv, Gen,
                                        "fuzz.cfg.ovchain.idiom");
  case 226:
  case 227:
  case 228:
  case 229:
  case 230:
  case 231:
  case 232:
  case 233:
    return emitRandomSelectLookupIdiom(B, A, Bv, Gen,
                                       "fuzz.cfg.lookup.idiom");
  case 234:
  case 235:
  case 236:
  case 237:
  case 238:
  case 239:
  case 240:
  case 241:
    return emitRandomNibbleReduceIdiom(B, M, A, Bv, Gen,
                                       "fuzz.cfg.nibble.idiom");
  default:
    switch (Gen() % 5) {
    case 0:
      return emitRandomNarrowVectorInstruction(B, M, A, Bv, Gen);
    case 1:
      return emitRandomVectorFPInstruction(B, M, A, Bv, Gen);
    case 2:
      return emitRandomVectorHalfFPInstruction(B, M, A, Bv, Gen);
    default:
      break;
    }
    return emitRandomVectorInstruction(B, M, A, Bv, Gen);
  }
}

Value *emitRandomCFGLinearArm(IRBuilder<NoFolder> &B, Module &M,
                              Value *Current, Value *Other,
                              std::minstd_rand &Gen) {
  Value *Result = Current;
  unsigned Steps = 1 + (Gen() % 6);
  for (unsigned I = 0; I < Steps; ++I) {
    Value *Bv = (Gen() % 2) == 0 ? Other : interestingI32(M.getContext(), Gen);
    Result = emitRandomCFGArmInstruction(B, M, Result, Bv, Gen);
  }
  return Result;
}

struct CFGFragment {
  Value *Result;
  BasicBlock *Tail;
};

unsigned chooseCFGDepth(const Function &F, unsigned HardMax,
                        std::minstd_rand &Gen) {
  unsigned MaxDepth = HardMax;
  if (F.size() >= MaxIRCFGBlocks - 160)
    MaxDepth = 1;
  else if (F.size() >= 1536)
    MaxDepth = std::min(MaxDepth, 2u);
  else if (F.size() >= 1024)
    MaxDepth = std::min(MaxDepth, 4u);
  else if (F.size() >= 768)
    MaxDepth = std::min(MaxDepth, 5u);
  else if (F.size() >= 512)
    MaxDepth = std::min(MaxDepth, 6u);
  else if (F.size() >= 256)
    MaxDepth = std::min(MaxDepth, 8u);

  unsigned Depth = 1;
  while (Depth < MaxDepth && (Gen() % 5) != 0)
    ++Depth;
  return Depth;
}

Value *chooseCFGValue(LLVMContext &Ctx, Value *Other, std::minstd_rand &Gen) {
  return (Gen() % 2) == 0 ? Other : interestingI32(Ctx, Gen);
}

bool canAddCFGBlocks(const Function &F, unsigned Blocks) {
  return F.size() + Blocks <= MaxIRCFGBlocks;
}

bool canGrowCFG(const Function &F) { return canAddCFGBlocks(F, 32); }

CFGFragment emitRandomCFGSubgraph(IRBuilder<NoFolder> &B, Module &M,
                                  BasicBlock *InsertBefore, Value *Current,
                                  Value *Other, unsigned Depth,
                                  std::minstd_rand &Gen);

CFGFragment emitRandomNestedDiamond(IRBuilder<NoFolder> &B, Module &M,
                                    BasicBlock *InsertBefore, Value *Current,
                                    Value *Other, unsigned Depth,
                                    std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Function *F = B.GetInsertBlock()->getParent();
  if (!canAddCFGBlocks(*F, 3))
    return {Current, B.GetInsertBlock()};
  BasicBlock *Then =
      BasicBlock::Create(Ctx, "fuzz.nested.then", F, InsertBefore);
  BasicBlock *Else =
      BasicBlock::Create(Ctx, "fuzz.nested.else", F, InsertBefore);
  BasicBlock *Join =
      BasicBlock::Create(Ctx, "fuzz.nested.join", F, InsertBefore);

  Value *Cond = B.CreateICmp(randomICmpPredicate(Gen), Current,
                             chooseCFGValue(Ctx, Other, Gen),
                             "fuzz.nested.branch");
  B.CreateCondBr(Cond, Then, Else);

  IRBuilder<NoFolder> ThenB(Then);
  CFGFragment ThenFrag =
      emitRandomCFGSubgraph(ThenB, M, Join, Current, Other, Depth - 1, Gen);
  IRBuilder<NoFolder> ThenTailB(ThenFrag.Tail);
  ThenTailB.CreateBr(Join);

  IRBuilder<NoFolder> ElseB(Else);
  CFGFragment ElseFrag =
      emitRandomCFGSubgraph(ElseB, M, Join, Current, Other, Depth - 1, Gen);
  IRBuilder<NoFolder> ElseTailB(ElseFrag.Tail);
  ElseTailB.CreateBr(Join);

  PHINode *Phi = PHINode::Create(I32, 2, "fuzz.nested.phi", Join->begin());
  Phi->addIncoming(ThenFrag.Result, ThenFrag.Tail);
  Phi->addIncoming(ElseFrag.Result, ElseFrag.Tail);
  return {Phi, Join};
}

CFGFragment emitRandomNestedCascade(IRBuilder<NoFolder> &B, Module &M,
                                    BasicBlock *InsertBefore, Value *Current,
                                    Value *Other, unsigned Depth,
                                    std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Function *F = B.GetInsertBlock()->getParent();
  BasicBlock *Head = B.GetInsertBlock();
  Value *Result = Current;
  unsigned Stages = 3 + (Gen() % 4);

  for (unsigned Stage = 0; Stage < Stages; ++Stage) {
    if (Depth == 0 || !canAddCFGBlocks(*F, 3))
      return {Result, Head};

    BasicBlock *Then =
        BasicBlock::Create(Ctx, "fuzz.cascade.then", F, InsertBefore);
    BasicBlock *Else =
        BasicBlock::Create(Ctx, "fuzz.cascade.else", F, InsertBefore);
    BasicBlock *Join =
        BasicBlock::Create(Ctx, "fuzz.cascade.join", F, InsertBefore);

    Value *StageOther = Stage == 0 ? Other : chooseCFGValue(Ctx, Other, Gen);
    IRBuilder<NoFolder> HeadB(Head);
    Value *Cond = HeadB.CreateICmp(randomICmpPredicate(Gen), Result,
                                   StageOther, "fuzz.cascade.branch");
    HeadB.CreateCondBr(Cond, Then, Else);

    IRBuilder<NoFolder> ThenB(Then);
    CFGFragment ThenFrag = emitRandomCFGSubgraph(
        ThenB, M, Join, Result, StageOther, Depth - 1, Gen);
    IRBuilder<NoFolder> ThenTailB(ThenFrag.Tail);
    ThenTailB.CreateBr(Join);

    IRBuilder<NoFolder> ElseB(Else);
    Value *ElseOther = chooseCFGValue(Ctx, StageOther, Gen);
    CFGFragment ElseFrag = emitRandomCFGSubgraph(
        ElseB, M, Join, Result, ElseOther, Depth - 1, Gen);
    IRBuilder<NoFolder> ElseTailB(ElseFrag.Tail);
    ElseTailB.CreateBr(Join);

    PHINode *Phi =
        PHINode::Create(I32, 2, "fuzz.cascade.phi", Join->begin());
    Phi->addIncoming(ThenFrag.Result, ThenFrag.Tail);
    Phi->addIncoming(ElseFrag.Result, ElseFrag.Tail);
    Result = Phi;
    Head = Join;
  }

  return {Result, Head};
}

CFGFragment emitRandomNestedSwitch(IRBuilder<NoFolder> &B, Module &M,
                                   BasicBlock *InsertBefore, Value *Current,
                                   Value *Other, unsigned Depth,
                                   std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Function *F = B.GetInsertBlock()->getParent();
  unsigned NumCases = 4 + (Gen() % 9);
  if (!canAddCFGBlocks(*F, NumCases + 2))
    return {Current, B.GetInsertBlock()};
  BasicBlock *Join =
      BasicBlock::Create(Ctx, "fuzz.nested.switch.join", F, InsertBefore);
  BasicBlock *Default =
      BasicBlock::Create(Ctx, "fuzz.nested.switch.default", F, Join);
  uint32_t Mask = NumCases <= 4 ? 3 : NumCases <= 8 ? 7 : 15;

  Value *Key = B.CreateAnd(Current, ci32(Ctx, Mask),
                           "fuzz.nested.switch.key");
  SwitchInst *Sw = B.CreateSwitch(Key, Default, NumCases);
  PHINode *Phi =
      PHINode::Create(I32, NumCases + 1, "fuzz.nested.switch.phi",
                      Join->begin());

  for (unsigned I = 0; I < NumCases; ++I) {
    BasicBlock *Case =
        BasicBlock::Create(Ctx, "fuzz.nested.switch.case", F, Join);
    Sw->addCase(ci32(Ctx, I), Case);

    IRBuilder<NoFolder> CaseB(Case);
    Value *CaseOther = I % 2 == 0 ? Other : interestingI32(Ctx, Gen);
    CFGFragment CaseFrag =
        emitRandomCFGSubgraph(CaseB, M, Join, Current, CaseOther, Depth - 1,
                              Gen);
    IRBuilder<NoFolder> CaseTailB(CaseFrag.Tail);
    CaseTailB.CreateBr(Join);
    Phi->addIncoming(CaseFrag.Result, CaseFrag.Tail);
  }

  IRBuilder<NoFolder> DefaultB(Default);
  CFGFragment DefaultFrag =
      emitRandomCFGSubgraph(DefaultB, M, Join, Current, Other, Depth - 1, Gen);
  IRBuilder<NoFolder> DefaultTailB(DefaultFrag.Tail);
  DefaultTailB.CreateBr(Join);
  Phi->addIncoming(DefaultFrag.Result, DefaultFrag.Tail);
  return {Phi, Join};
}

CFGFragment emitRandomNestedCountedLoop(IRBuilder<NoFolder> &B, Module &M,
                                        BasicBlock *InsertBefore,
                                        Value *Current, Value *Other,
                                        unsigned Depth,
                                        std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Function *F = B.GetInsertBlock()->getParent();
  bool UseEarlyExit = (Gen() % 4) == 0;
  if (!canAddCFGBlocks(*F, UseEarlyExit ? 4 : 3))
    return {Current, B.GetInsertBlock()};
  BasicBlock *Preheader = B.GetInsertBlock();
  BasicBlock *Header =
      BasicBlock::Create(Ctx, "fuzz.nested.loop.header", F, InsertBefore);
  BasicBlock *Body =
      BasicBlock::Create(Ctx, "fuzz.nested.loop.body", F, InsertBefore);
  BasicBlock *Exit =
      BasicBlock::Create(Ctx, "fuzz.nested.loop.exit", F, InsertBefore);

  Value *TripCount = nullptr;
  if ((Gen() % 2) == 0) {
    TripCount = ci32(Ctx, 1 + (Gen() % 4));
  } else {
    Value *TripSeed =
        (Gen() % 2) == 0 ? Current : chooseCFGValue(Ctx, Other, Gen);
    Value *Masked =
        B.CreateAnd(TripSeed, ci32(Ctx, 3), "fuzz.loop.trip.inner.mask");
    TripCount = B.CreateAdd(Masked, ci32(Ctx, 1), "fuzz.loop.trip.inner");
  }
  B.CreateBr(Header);

  IRBuilder<NoFolder> HeaderB(Header);
  PHINode *Index = HeaderB.CreatePHI(I32, 2, "fuzz.loop.iv.inner");
  PHINode *Acc = HeaderB.CreatePHI(I32, 2, "fuzz.loop.acc.inner");
  Index->addIncoming(ci32(Ctx, 0), Preheader);
  Acc->addIncoming(Current, Preheader);
  Value *Cond = HeaderB.CreateICmpULT(Index, TripCount,
                                      "fuzz.loop.cond.inner");
  HeaderB.CreateCondBr(Cond, Body, Exit);

  IRBuilder<NoFolder> BodyB(Body);
  CFGFragment BodyFrag =
      emitRandomCFGSubgraph(BodyB, M, Exit, Acc, Other, Depth - 1, Gen);

  Value *NextAcc = BodyFrag.Result;
  Value *ExitResult = Acc;
  BasicBlock *Backedge = BodyFrag.Tail;
  if (UseEarlyExit) {
    IRBuilder<NoFolder> BreakB(BodyFrag.Tail);
    Value *BreakCond =
        BreakB.CreateICmp(randomICmpPredicate(Gen), NextAcc,
                          chooseCFGValue(Ctx, Other, Gen),
                          "fuzz.loop.break.inner");
    Backedge =
        BasicBlock::Create(Ctx, "fuzz.nested.loop.continue", F, Exit);
    BreakB.CreateCondBr(BreakCond, Exit, Backedge);

    PHINode *ExitPhi =
        PHINode::Create(I32, 2, "fuzz.loop.exit.value.inner", Exit->begin());
    ExitPhi->addIncoming(Acc, Header);
    ExitPhi->addIncoming(NextAcc, BodyFrag.Tail);
    ExitResult = ExitPhi;
  }

  IRBuilder<NoFolder> BackedgeB(Backedge);
  if ((Gen() % 2) == 0)
    NextAcc = BackedgeB.CreateXor(NextAcc, Index, "fuzz.loop.acc.inner.mix");
  Value *NextIndex =
      BackedgeB.CreateAdd(Index, ci32(Ctx, 1), "fuzz.loop.next.inner");
  BackedgeB.CreateBr(Header);
  Index->addIncoming(NextIndex, Backedge);
  Acc->addIncoming(NextAcc, Backedge);

  return {ExitResult, Exit};
}

CFGFragment emitRandomCFGSubgraph(IRBuilder<NoFolder> &B, Module &M,
                                  BasicBlock *InsertBefore, Value *Current,
                                  Value *Other, unsigned Depth,
                                  std::minstd_rand &Gen) {
  Value *Linear = emitRandomCFGLinearArm(B, M, Current, Other, Gen);
  Function *F = B.GetInsertBlock()->getParent();
  if (Depth == 0 || !canGrowCFG(*F) || (Gen() % 7) == 0)
    return {Linear, B.GetInsertBlock()};

  Value *NestedOther = chooseCFGValue(M.getContext(), Other, Gen);
  switch (Gen() % 7) {
  case 0:
    return emitRandomNestedSwitch(B, M, InsertBefore, Linear, NestedOther,
                                  Depth, Gen);
  case 1:
    return emitRandomNestedCascade(B, M, InsertBefore, Linear, NestedOther,
                                   Depth, Gen);
  case 2:
    if (Depth <= 2)
      return emitRandomNestedCountedLoop(B, M, InsertBefore, Linear,
                                         NestedOther, Depth, Gen);
    [[fallthrough]];
  default:
    return emitRandomNestedDiamond(B, M, InsertBefore, Linear, NestedOther,
                                   Depth, Gen);
  }
}

void mutateIRAddDiamond(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F || !canGrowCFG(*F))
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;

  Value *Current = Store->getValueOperand();
  Value *Other = chooseI32Value(Store, Gen);
  BasicBlock *Head = Store->getParent();
  BasicBlock *Join = Head->splitBasicBlock(Store->getIterator(), "fuzz.join");
  BasicBlock *Then = BasicBlock::Create(M.getContext(), "fuzz.then", F, Join);
  BasicBlock *Else = BasicBlock::Create(M.getContext(), "fuzz.else", F, Join);

  Instruction *OldTerm = Head->getTerminator();
  IRBuilder<NoFolder> HeadB(OldTerm);
  Value *Cond =
      HeadB.CreateICmp(randomICmpPredicate(Gen), Current, Other, "fuzz.branch");
  HeadB.CreateCondBr(Cond, Then, Else);
  OldTerm->eraseFromParent();

  IRBuilder<NoFolder> ThenB(Then);
  CFGFragment ThenFrag = emitRandomCFGSubgraph(
      ThenB, M, Join, Current, Other, chooseCFGDepth(*F, 10, Gen), Gen);
  IRBuilder<NoFolder> ThenTailB(ThenFrag.Tail);
  ThenTailB.CreateBr(Join);

  IRBuilder<NoFolder> ElseB(Else);
  CFGFragment ElseFrag = emitRandomCFGSubgraph(
      ElseB, M, Join, Current, Other, chooseCFGDepth(*F, 10, Gen), Gen);
  IRBuilder<NoFolder> ElseTailB(ElseFrag.Tail);
  ElseTailB.CreateBr(Join);

  PHINode *Phi = PHINode::Create(Type::getInt32Ty(M.getContext()), 2,
                                 "fuzz.phi", Join->begin());
  Phi->addIncoming(ThenFrag.Result, ThenFrag.Tail);
  Phi->addIncoming(ElseFrag.Result, ElseFrag.Tail);
  Store->setOperand(0, Phi);
}

void mutateIRAddSwitch(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F || !canGrowCFG(*F))
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;

  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Current = Store->getValueOperand();
  Value *Other = chooseI32Value(Store, Gen);
  unsigned NumCases = 4 + (Gen() % 9);
  uint32_t Mask = NumCases <= 4 ? 3 : NumCases <= 8 ? 7 : 15;

  BasicBlock *Head = Store->getParent();
  BasicBlock *Join =
      Head->splitBasicBlock(Store->getIterator(), "fuzz.switch.join");
  BasicBlock *Default =
      BasicBlock::Create(Ctx, "fuzz.switch.default", F, Join);

  Instruction *OldTerm = Head->getTerminator();
  IRBuilder<NoFolder> HeadB(OldTerm);
  Value *Key = HeadB.CreateAnd(Current, ci32(Ctx, Mask), "fuzz.switch.key");
  SwitchInst *Sw = HeadB.CreateSwitch(Key, Default, NumCases);
  OldTerm->eraseFromParent();

  PHINode *Phi = PHINode::Create(I32, NumCases + 1, "fuzz.switch.phi",
                                 Join->begin());
  for (unsigned I = 0; I < NumCases; ++I) {
    BasicBlock *Case =
        BasicBlock::Create(Ctx, "fuzz.switch.case", F, Join);
    Sw->addCase(ci32(Ctx, I), Case);

    IRBuilder<NoFolder> CaseB(Case);
    Value *CaseOther =
        (I % 2) == 0 ? Other : ci32(Ctx, randomInteresting64(Gen));
    CFGFragment CaseFrag = emitRandomCFGSubgraph(
        CaseB, M, Join, Current, CaseOther, chooseCFGDepth(*F, 9, Gen), Gen);
    IRBuilder<NoFolder> CaseTailB(CaseFrag.Tail);
    CaseTailB.CreateBr(Join);
    Phi->addIncoming(CaseFrag.Result, CaseFrag.Tail);
  }

  IRBuilder<NoFolder> DefaultB(Default);
  CFGFragment DefaultFrag = emitRandomCFGSubgraph(
      DefaultB, M, Join, Current, Other, chooseCFGDepth(*F, 9, Gen), Gen);
  IRBuilder<NoFolder> DefaultTailB(DefaultFrag.Tail);
  DefaultTailB.CreateBr(Join);
  Phi->addIncoming(DefaultFrag.Result, DefaultFrag.Tail);
  Store->setOperand(0, Phi);
}

void mutateIRAddCountedLoop(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F || !canGrowCFG(*F))
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;

  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Current = Store->getValueOperand();
  Value *Other = chooseI32Value(Store, Gen);
  bool UseSecondAccumulator = (Gen() % 3) == 0;
  bool UseEarlyExit = (Gen() % 4) == 0;

  BasicBlock *Preheader = Store->getParent();
  BasicBlock *Exit =
      Preheader->splitBasicBlock(Store->getIterator(), "fuzz.loop.exit");
  BasicBlock *Header =
      BasicBlock::Create(Ctx, "fuzz.loop.header", F, Exit);
  BasicBlock *Body = BasicBlock::Create(Ctx, "fuzz.loop.body", F, Exit);

  Instruction *OldTerm = Preheader->getTerminator();
  IRBuilder<NoFolder> PreB(OldTerm);
  Value *TripCount = nullptr;
  if ((Gen() % 2) == 0) {
    TripCount = ci32(Ctx, 1 + (Gen() % 16));
  } else {
    Value *TripSeed = (Gen() % 2) == 0 ? Current : Other;
    Value *Masked =
        PreB.CreateAnd(TripSeed, ci32(Ctx, 15), "fuzz.loop.trip.mask");
    TripCount = PreB.CreateAdd(Masked, ci32(Ctx, 1), "fuzz.loop.trip");
  }
  PreB.CreateBr(Header);
  OldTerm->eraseFromParent();

  IRBuilder<NoFolder> HeaderB(Header);
  PHINode *Index = HeaderB.CreatePHI(I32, 2, "fuzz.loop.iv");
  PHINode *Acc = HeaderB.CreatePHI(I32, 2, "fuzz.loop.acc");
  PHINode *Acc2 = nullptr;
  Index->addIncoming(ci32(Ctx, 0), Preheader);
  Acc->addIncoming(Current, Preheader);
  if (UseSecondAccumulator) {
    Acc2 = HeaderB.CreatePHI(I32, 2, "fuzz.loop.acc2");
    Acc2->addIncoming(Other, Preheader);
  }
  Value *Done =
      HeaderB.CreateICmpULT(Index, TripCount, "fuzz.loop.cond");
  HeaderB.CreateCondBr(Done, Body, Exit);

  IRBuilder<NoFolder> BodyB(Body);
  CFGFragment BodyFrag = emitRandomCFGSubgraph(
      BodyB, M, Exit, Acc, Other, chooseCFGDepth(*F, 6, Gen), Gen);
  CFGFragment FinalFrag = BodyFrag;
  Value *NextAcc2 = nullptr;
  if (Acc2) {
    IRBuilder<NoFolder> Acc2B(BodyFrag.Tail);
    FinalFrag = emitRandomCFGSubgraph(Acc2B, M, Exit, Acc2, BodyFrag.Result,
                                      chooseCFGDepth(*F, 5, Gen), Gen);
    IRBuilder<NoFolder> MixB(FinalFrag.Tail);
    NextAcc2 = MixB.CreateXor(FinalFrag.Result, BodyFrag.Result,
                              "fuzz.loop.acc2.mix");
  }

  Value *NextAcc = BodyFrag.Result;
  Value *BreakValue = nullptr;
  BasicBlock *Backedge = FinalFrag.Tail;
  if (UseEarlyExit) {
    IRBuilder<NoFolder> BreakB(FinalFrag.Tail);
    BreakValue = NextAcc;
    if (Acc2)
      BreakValue =
          BreakB.CreateAdd(NextAcc, NextAcc2, "fuzz.loop.break.value");
    Value *BreakCond = BreakB.CreateICmp(randomICmpPredicate(Gen), BreakValue,
                                         chooseCFGValue(Ctx, Other, Gen),
                                         "fuzz.loop.break");
    Backedge = BasicBlock::Create(Ctx, "fuzz.loop.continue", F, Exit);
    BreakB.CreateCondBr(BreakCond, Exit, Backedge);
  }

  IRBuilder<NoFolder> BodyTailB(Backedge);
  Value *NextIndex =
      BodyTailB.CreateAdd(Index, ci32(Ctx, 1), "fuzz.loop.next");
  BodyTailB.CreateBr(Header);
  Index->addIncoming(NextIndex, Backedge);
  Acc->addIncoming(NextAcc, Backedge);
  if (Acc2)
    Acc2->addIncoming(NextAcc2, Backedge);

  if (UseEarlyExit) {
    Value *NaturalExitValue = Acc;
    if (Acc2) {
      IRBuilder<NoFolder> HeaderExitB(Header->getTerminator());
      NaturalExitValue =
          HeaderExitB.CreateAdd(Acc, Acc2, "fuzz.loop.natural.value");
    }
    PHINode *ExitValue =
        PHINode::Create(I32, 2, "fuzz.loop.exit.value", Exit->begin());
    ExitValue->addIncoming(NaturalExitValue, Header);
    ExitValue->addIncoming(BreakValue, FinalFrag.Tail);
    Store->setOperand(0, ExitValue);
  } else if (Acc2) {
    IRBuilder<NoFolder> ExitB(Store);
    Value *Mixed = ExitB.CreateAdd(Acc, Acc2, "fuzz.loop.acc.mix");
    Store->setOperand(0, Mixed);
  } else {
    Store->setOperand(0, Acc);
  }
}

void mutateIRAddLoopNest(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F || !canGrowCFG(*F))
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;

  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Current = Store->getValueOperand();
  Value *Other = chooseI32Value(Store, Gen);

  BasicBlock *Preheader = Store->getParent();
  BasicBlock *Exit =
      Preheader->splitBasicBlock(Store->getIterator(), "fuzz.loop.nest.exit");
  BasicBlock *Header =
      BasicBlock::Create(Ctx, "fuzz.loop.nest.header", F, Exit);
  BasicBlock *Body =
      BasicBlock::Create(Ctx, "fuzz.loop.nest.body", F, Exit);

  Instruction *OldTerm = Preheader->getTerminator();
  IRBuilder<NoFolder> PreB(OldTerm);
  Value *TripCount = nullptr;
  if ((Gen() % 2) == 0) {
    TripCount = ci32(Ctx, 1 + (Gen() % 4));
  } else {
    Value *TripSeed = (Gen() % 2) == 0 ? Current : Other;
    Value *Masked =
        PreB.CreateAnd(TripSeed, ci32(Ctx, 3), "fuzz.loop.nest.trip.mask");
    TripCount = PreB.CreateAdd(Masked, ci32(Ctx, 1), "fuzz.loop.nest.trip");
  }
  PreB.CreateBr(Header);
  OldTerm->eraseFromParent();

  IRBuilder<NoFolder> HeaderB(Header);
  PHINode *Index = HeaderB.CreatePHI(I32, 2, "fuzz.loop.nest.iv");
  PHINode *Acc = HeaderB.CreatePHI(I32, 2, "fuzz.loop.nest.acc");
  Index->addIncoming(ci32(Ctx, 0), Preheader);
  Acc->addIncoming(Current, Preheader);
  Value *Cond =
      HeaderB.CreateICmpULT(Index, TripCount, "fuzz.loop.nest.cond");
  HeaderB.CreateCondBr(Cond, Body, Exit);

  IRBuilder<NoFolder> BodyB(Body);
  unsigned InnerDepth = std::min(chooseCFGDepth(*F, 5, Gen), 3u);
  CFGFragment Frag =
      emitRandomNestedCountedLoop(BodyB, M, Exit, Acc, Other, InnerDepth, Gen);
  if ((Gen() % 2) == 0 && canGrowCFG(*F)) {
    IRBuilder<NoFolder> TailB(Frag.Tail);
    unsigned TailDepth = std::min(chooseCFGDepth(*F, 4, Gen), 3u);
    Frag = emitRandomCFGSubgraph(TailB, M, Exit, Frag.Result, Other, TailDepth,
                                 Gen);
  }

  IRBuilder<NoFolder> BackedgeB(Frag.Tail);
  Value *NextAcc = Frag.Result;
  switch (Gen() % 4) {
  case 0:
    NextAcc = BackedgeB.CreateXor(NextAcc, Index, "fuzz.loop.nest.acc.xor");
    break;
  case 1:
    NextAcc = BackedgeB.CreateAdd(NextAcc, Other, "fuzz.loop.nest.acc.add");
    break;
  default:
    break;
  }
  Value *NextIndex =
      BackedgeB.CreateAdd(Index, ci32(Ctx, 1), "fuzz.loop.nest.next");
  BackedgeB.CreateBr(Header);
  Index->addIncoming(NextIndex, Frag.Tail);
  Acc->addIncoming(NextAcc, Frag.Tail);

  Store->setOperand(0, Acc);
}

void mutateIRAddMultiExitLoop(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F || !canAddCFGBlocks(*F, 6))
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;

  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *Current = Store->getValueOperand();
  Value *Other = chooseI32Value(Store, Gen);

  BasicBlock *Preheader = Store->getParent();
  BasicBlock *Exit =
      Preheader->splitBasicBlock(Store->getIterator(), "fuzz.loop.multi.exit");
  BasicBlock *Header =
      BasicBlock::Create(Ctx, "fuzz.loop.multi.header", F, Exit);
  BasicBlock *Body =
      BasicBlock::Create(Ctx, "fuzz.loop.multi.body", F, Exit);
  BasicBlock *BreakA =
      BasicBlock::Create(Ctx, "fuzz.loop.multi.break.a", F, Exit);
  BasicBlock *BreakB =
      BasicBlock::Create(Ctx, "fuzz.loop.multi.break.b", F, Exit);
  BasicBlock *Continue =
      BasicBlock::Create(Ctx, "fuzz.loop.multi.continue", F, Exit);

  Instruction *OldTerm = Preheader->getTerminator();
  IRBuilder<NoFolder> PreB(OldTerm);
  Value *TripCount = nullptr;
  if ((Gen() % 2) == 0) {
    TripCount = ci32(Ctx, 1 + (Gen() % 8));
  } else {
    Value *TripSeed = (Gen() % 2) == 0 ? Current : Other;
    Value *Masked =
        PreB.CreateAnd(TripSeed, ci32(Ctx, 7), "fuzz.loop.multi.trip.mask");
    TripCount = PreB.CreateAdd(Masked, ci32(Ctx, 1),
                               "fuzz.loop.multi.trip");
  }
  PreB.CreateBr(Header);
  OldTerm->eraseFromParent();

  IRBuilder<NoFolder> HeaderB(Header);
  PHINode *Index = HeaderB.CreatePHI(I32, 2, "fuzz.loop.iv.multi");
  PHINode *Acc = HeaderB.CreatePHI(I32, 2, "fuzz.loop.acc.multi");
  Index->addIncoming(ci32(Ctx, 0), Preheader);
  Acc->addIncoming(Current, Preheader);
  Value *Cond =
      HeaderB.CreateICmpULT(Index, TripCount, "fuzz.loop.multi.cond");
  HeaderB.CreateCondBr(Cond, Body, Exit);

  IRBuilder<NoFolder> BodyB(Body);
  CFGFragment BodyFrag = emitRandomCFGSubgraph(
      BodyB, M, Exit, Acc, Other, chooseCFGDepth(*F, 6, Gen), Gen);

  IRBuilder<NoFolder> TailB(BodyFrag.Tail);
  Value *Key = TailB.CreateAnd(BodyFrag.Result, ci32(Ctx, 3),
                               "fuzz.loop.multi.exit.key");
  SwitchInst *Sw = TailB.CreateSwitch(Key, Continue, 2);
  Sw->addCase(ci32(Ctx, 0), BreakA);
  Sw->addCase(ci32(Ctx, 1), BreakB);

  IRBuilder<NoFolder> BreakAB(BreakA);
  Value *BreakAValue =
      BreakAB.CreateXor(BodyFrag.Result, Index, "fuzz.loop.multi.break.a.val");
  BreakAB.CreateBr(Exit);

  IRBuilder<NoFolder> BreakBB(BreakB);
  Value *BreakBValue =
      BreakBB.CreateAdd(BodyFrag.Result, Other, "fuzz.loop.multi.break.b.val");
  BreakBB.CreateBr(Exit);

  IRBuilder<NoFolder> ContinueB(Continue);
  Value *NextAcc =
      ContinueB.CreateXor(BodyFrag.Result, Other, "fuzz.loop.multi.acc.next");
  Value *NextIndex =
      ContinueB.CreateAdd(Index, ci32(Ctx, 1), "fuzz.loop.multi.next");
  ContinueB.CreateBr(Header);
  Index->addIncoming(NextIndex, Continue);
  Acc->addIncoming(NextAcc, Continue);

  PHINode *ExitValue =
      PHINode::Create(I32, 3, "fuzz.loop.multi.exit.value", Exit->begin());
  ExitValue->addIncoming(Acc, Header);
  ExitValue->addIncoming(BreakAValue, BreakA);
  ExitValue->addIncoming(BreakBValue, BreakB);
  Store->setOperand(0, ExitValue);
}

void mutateIRAddCascade(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F || !canGrowCFG(*F))
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;

  Value *Current = Store->getValueOperand();
  Value *Other = chooseI32Value(Store, Gen);
  BasicBlock *Head = Store->getParent();
  BasicBlock *Join =
      Head->splitBasicBlock(Store->getIterator(), "fuzz.cascade.outer.join");

  Instruction *OldTerm = Head->getTerminator();
  IRBuilder<NoFolder> HeadB(OldTerm);
  CFGFragment Frag = emitRandomNestedCascade(
      HeadB, M, Join, Current, Other, chooseCFGDepth(*F, 9, Gen), Gen);
  IRBuilder<NoFolder> TailB(Frag.Tail);
  TailB.CreateBr(Join);
  OldTerm->eraseFromParent();
  Store->setOperand(0, Frag.Result);
}

void mutateIRAddComplexCFG(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F || !canGrowCFG(*F))
    return;
  StoreInst *Store = findIRResultStore(*F);
  if (!Store)
    return;

  LLVMContext &Ctx = M.getContext();
  Value *Current = Store->getValueOperand();
  Value *BaseOther = chooseI32Value(Store, Gen);
  BasicBlock *Head = Store->getParent();
  BasicBlock *Join =
      Head->splitBasicBlock(Store->getIterator(), "fuzz.complex.join");

  Instruction *OldTerm = Head->getTerminator();
  CFGFragment Frag{Current, Head};
  unsigned Sections = 2 + (Gen() % 4);
  for (unsigned I = 0; I < Sections && canGrowCFG(*F); ++I) {
    unsigned Depth = chooseCFGDepth(*F, 10, Gen);
    Value *Other = I == 0 ? BaseOther : chooseCFGValue(Ctx, BaseOther, Gen);
    CFGFragment Next{Frag.Result, Frag.Tail};

    auto EmitSection = [&](IRBuilder<NoFolder> &SectionB) {
      switch (Gen() % 5) {
      case 0:
        Next = emitRandomNestedSwitch(SectionB, M, Join, Frag.Result, Other,
                                      Depth, Gen);
        break;
      case 1:
        Next = emitRandomNestedCascade(SectionB, M, Join, Frag.Result, Other,
                                       Depth, Gen);
        break;
      case 2:
        Next = emitRandomNestedCountedLoop(SectionB, M, Join, Frag.Result,
                                           Other, std::min(Depth, 6u), Gen);
        break;
      default:
        Next = emitRandomCFGSubgraph(SectionB, M, Join, Frag.Result, Other,
                                     Depth, Gen);
        break;
      }
    };

    if (Frag.Tail == Head) {
      IRBuilder<NoFolder> SectionB(OldTerm);
      EmitSection(SectionB);
    } else {
      IRBuilder<NoFolder> SectionB(Frag.Tail);
      EmitSection(SectionB);
    }
    Frag = Next;
  }

  if (Frag.Tail == Head) {
    IRBuilder<NoFolder> TailB(OldTerm);
    TailB.CreateBr(Join);
  } else {
    IRBuilder<NoFolder> TailB(Frag.Tail);
    TailB.CreateBr(Join);
  }
  OldTerm->eraseFromParent();
  Store->setOperand(0, Frag.Result);
}

void mutateIRModifyConstant(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F)
    return;
  SmallVector<std::pair<Instruction *, unsigned>, 32> Candidates;
  for (BasicBlock &BB : *F) {
    for (Instruction &I : BB) {
      if (!I.getName().starts_with("fuzz."))
        continue;
      if (I.getName().starts_with("fuzz.loop."))
        continue;
      if (I.getName().starts_with("fuzz.vec."))
        continue;
      if (I.getName().starts_with("fuzz.fp.") ||
          I.getName().starts_with("fuzz.fp16.") ||
          I.getName().starts_with("fuzz.cfg.fp.") ||
          I.getName().starts_with("fuzz.cfg.fp16."))
        continue;
      if (I.getName().starts_with("fuzz.sdiv.") ||
          I.getName().starts_with("fuzz.srem.") ||
          I.getName().starts_with("fuzz.cfg.sdiv.") ||
          I.getName().starts_with("fuzz.cfg.srem."))
        continue;
      for (unsigned IOp = 0; IOp < I.getNumOperands(); ++IOp) {
        if (auto *Call = dyn_cast<CallBase>(&I)) {
          if (IOp == Call->arg_size())
            continue;
          if (IOp < Call->arg_size() && Call->paramHasAttr(IOp, Attribute::ImmArg))
            continue;
        }
        if (isa<ConstantInt>(I.getOperand(IOp)) &&
            I.getOperand(IOp)->getType()->isIntegerTy())
          Candidates.push_back({&I, IOp});
      }
    }
  }
  if (Candidates.empty())
    return;
  auto [I, IOp] = Candidates[Gen() % Candidates.size()];
  Type *Ty = I->getOperand(IOp)->getType();
  if ((I->getOpcode() == Instruction::Shl ||
       I->getOpcode() == Instruction::LShr ||
       I->getOpcode() == Instruction::AShr) &&
      IOp == 1) {
    if (unsigned Width = integerScalarWidth(I->getType()))
      I->setOperand(IOp, ConstantInt::get(Ty, Gen() % Width));
  } else if (Ty->isIntegerTy(1))
    I->setOperand(IOp, ConstantInt::get(Ty, Gen() & 1));
  else
    I->setOperand(IOp, ConstantInt::get(Ty, randomInteresting64(Gen)));
}

void mutateIRRemoveInstruction(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F)
    return;
  SmallVector<Instruction *, 32> Candidates;
  for (BasicBlock &BB : *F) {
    for (Instruction &I : BB) {
      if (I.isTerminator() || I.mayReadOrWriteMemory() || I.getType()->isVoidTy())
        continue;
      if (I.use_empty())
        Candidates.push_back(&I);
    }
  }
  if (Candidates.empty())
    return;
  Candidates[Gen() % Candidates.size()]->eraseFromParent();
}

void mutateIRModule(Module &M, std::minstd_rand &Gen) {
  unsigned NumMutations = 1;
  while (NumMutations < 32 && (Gen() % 3) == 0)
    ++NumMutations;
  for (unsigned I = 0; I < NumMutations; ++I) {
    switch (Gen() % 36) {
    case 0:
    case 1:
    case 2:
      mutateIRAddInstruction(M, Gen);
      break;
    case 3:
    case 4:
    case 5:
    case 6:
      mutateIRAddDiamond(M, Gen);
      break;
    case 7:
    case 8:
    case 9:
    case 10:
      mutateIRAddSwitch(M, Gen);
      break;
    case 11:
    case 12:
    case 13:
    case 14:
      mutateIRAddCountedLoop(M, Gen);
      break;
    case 15:
    case 16:
    case 17:
    case 18:
      mutateIRAddCascade(M, Gen);
      break;
    case 19:
    case 20:
    case 21:
    case 22:
    case 23:
      mutateIRAddComplexCFG(M, Gen);
      break;
    case 24:
    case 25:
    case 26:
    case 27:
      mutateIRAddLoopNest(M, Gen);
      break;
    case 28:
    case 29:
    case 30:
    case 31:
      mutateIRAddMultiExitLoop(M, Gen);
      break;
    case 32:
    case 33:
      mutateIRModifyConstant(M, Gen);
      break;
    case 34:
    case 35:
    default:
      mutateIRRemoveInstruction(M, Gen);
      break;
    }
  }
  scrubPoisonAnnotations(M);
}

void prepareIRModuleForCompile(Module &M, StringRef CPU, StringRef KernelName) {
  M.setTargetTriple(Triple("amdgcn-amd-amdhsa"));
  M.setDataLayout(DataLayout);
  addModuleFlagIfMissing(M, Module::Error, "amdhsa_code_object_version", 600);
  addModuleFlagIfMissing(M, Module::Error, "amdgpu_printf_kind", "hostcall");
  addModuleFlagIfMissing(M, Module::Max, "PIC Level", 2);
  if (Function *F = findIRKernel(M)) {
    F->setName(KernelName);
    F->setCallingConv(CallingConv::AMDGPU_KERNEL);
    F->setVisibility(GlobalValue::ProtectedVisibility);
    F->addFnAttr(Attribute::Convergent);
    F->addFnAttr(Attribute::NoUnwind);
    F->addFnAttr("amdgpu-flat-work-group-size", "1,256");
    F->addFnAttr("target-cpu", CPU);
    F->addFnAttr("uniform-work-group-size", "true");
  }
}

CompileResult
compileIRModuleToObject(const Module &Input, StringRef CPU,
                        OptimizationLevel Level, StringRef KernelName,
                        std::string *IR = nullptr) {
  CompileResult Result;
  TargetMachine *TM = getTargetMachine(CPU, Level);
  std::unique_ptr<Module> M = CloneModule(Input);
  prepareIRModuleForCompile(*M, CPU, KernelName);
  M->setDataLayout(TM->createDataLayout());
  if (IR)
    *IR = moduleToString(*M);
  if (!validateIRCorpusModule(*M)) {
    Result.FailureStage = "validate";
    return Result;
  }

  bool PipelineOk = false;
  CrashRecoveryContext PipelineCRC;
  if (!PipelineCRC.RunSafely(
          [&] { PipelineOk = runOptimizationPipeline(*M, *TM, Level); })) {
    Result.FailureStage = "opt-pipeline";
    Result.Crashed = true;
    Result.CrashRetCode = PipelineCRC.RetCode;
    return Result;
  }
  if (!PipelineOk) {
    Result.FailureStage = "opt-pipeline";
    return Result;
  }

  std::optional<SmallVector<char, 0>> Obj;
  CrashRecoveryContext CodeGenCRC;
  if (!CodeGenCRC.RunSafely([&] { Obj = emitObject(*M, *TM); })) {
    Result.FailureStage = "codegen";
    Result.Crashed = true;
    Result.CrashRetCode = CodeGenCRC.RetCode;
    return Result;
  }
  if (!Obj) {
    Result.FailureStage = "codegen";
    return Result;
  }

  Result.Object = std::move(*Obj);
  Result.Success = true;
  return Result;
}

std::vector<uint8_t> moduleToBitcode(Module &M) {
  SmallVector<char, 0> Buffer;
  raw_svector_ostream OS(Buffer);
  WriteBitcodeToFile(M, OS);
  return std::vector<uint8_t>(Buffer.begin(), Buffer.end());
}

bool isCrossOverCloneableInstruction(const Instruction &I) {
  if (I.isTerminator() || I.mayReadOrWriteMemory() ||
      isa<GetElementPtrInst, PHINode>(&I))
    return false;
  if (I.hasName() && I.getName().starts_with("fuzz.load."))
    return false;
  Type *Ty = I.getType();
  if (!Ty->isIntegerTy(1) && !Ty->isIntegerTy(32))
    return false;
  return isAllowedIRInstruction(I);
}

bool cloneInstructionIntoKernel(Instruction *OtherI, Module &Base,
                                StoreInst *Store,
                                DenseMap<const Value *, Value *> &Map,
                                std::minstd_rand &Gen) {
  if (!OtherI || !isCrossOverCloneableInstruction(*OtherI))
    return false;
  Function *BaseF = findIRKernel(Base);
  if (!BaseF)
    return false;

  Instruction *Clone = OtherI->clone();
  for (unsigned I = 0; I < Clone->getNumOperands(); ++I) {
    Value *Op = Clone->getOperand(I);
    if (Value *Mapped = Map.lookup(Op)) {
      Clone->setOperand(I, Mapped);
      continue;
    }
    if (auto *C = dyn_cast<Constant>(Op)) {
      Clone->setOperand(I, C);
      continue;
    }
    if (auto *F = dyn_cast<Function>(Op)) {
      if (!F->isIntrinsic()) {
        Clone->deleteValue();
        return false;
      }
      FunctionCallee NewF =
          Base.getOrInsertFunction(F->getName(), F->getFunctionType());
      Clone->setOperand(I, cast<Function>(NewF.getCallee()));
      continue;
    }
    if (Op->getType()->isIntegerTy(32)) {
      Clone->setOperand(I, chooseI32Value(Store, Gen));
      continue;
    }
    Clone->deleteValue();
    return false;
  }
  Clone->insertBefore(Store->getIterator());
  Map[OtherI] = Clone;
  return true;
}

bool crossOverIRModules(Module &Base, const Module &Other,
                        std::minstd_rand &Gen) {
  Function *BaseF = findIRKernel(Base);
  const Function *OtherF = nullptr;
  for (const Function &F : Other) {
    if ((F.getName() == "fuzz_kernel" || F.getName() == "fuzz_kernel_o0" ||
         F.getName() == "fuzz_kernel_o2") &&
        !F.isDeclaration()) {
      OtherF = &F;
      break;
    }
  }
  if (!BaseF || !OtherF)
    return false;
  StoreInst *Store = findIRResultStore(*BaseF);
  if (!Store)
    return false;

  SmallVector<const Instruction *, 64> OtherI32;
  for (const BasicBlock &BB : *OtherF)
    for (const Instruction &I : BB)
      if (I.getType()->isIntegerTy(32) && isCrossOverCloneableInstruction(I))
        OtherI32.push_back(&I);
  if (OtherI32.empty())
    return false;

  const Instruction *Chosen = OtherI32[Gen() % OtherI32.size()];
  SmallVector<const Instruction *, 16> ToClone;
  for (const BasicBlock &BB : *OtherF) {
    for (const Instruction &I : BB) {
      if (&I == Chosen || ToClone.size() < 8) {
        if (isCrossOverCloneableInstruction(I))
          ToClone.push_back(&I);
      }
      if (&I == Chosen)
        break;
    }
  }

  DenseMap<const Value *, Value *> Map;
  for (const Argument &A : OtherF->args())
    Map[&A] = BaseF->getArg(A.getArgNo());
  Value *Last = nullptr;
  for (const Instruction *I : ToClone) {
    if (!cloneInstructionIntoKernel(const_cast<Instruction *>(I), Base, Store,
                                    Map, Gen))
      continue;
    Last = Map.lookup(I);
  }
  if (!Last || !Last->getType()->isIntegerTy(32))
    return false;

  IRBuilder<NoFolder> B(Store);
  Value *Mixed = B.CreateXor(Store->getValueOperand(), Last, "fuzz.xover");
  Store->setOperand(0, Mixed);
  scrubPoisonAnnotations(Base);
  return true;
}

bool typeUnsupportedByInterpreterOracle(Type *Ty) {
  if (!Ty)
    return false;
  if (Ty->isFloatingPointTy())
    return true;
  if (auto *VecTy = dyn_cast<VectorType>(Ty)) {
    Type *EltTy = VecTy->getElementType();
    return EltTy->isFloatingPointTy();
  }
  if (auto *StructTy = dyn_cast<StructType>(Ty))
    for (Type *EltTy : StructTy->elements())
      if (typeUnsupportedByInterpreterOracle(EltTy))
        return true;
  return false;
}

bool intrinsicUnsupportedByInterpreterOracle(Intrinsic::ID ID) {
  switch (ID) {
  case Intrinsic::amdgcn_workgroup_id_x:
  case Intrinsic::amdgcn_workitem_id_x:
  case Intrinsic::ctlz:
  case Intrinsic::cttz:
  case Intrinsic::ctpop:
  case Intrinsic::bitreverse:
  case Intrinsic::bswap:
  case Intrinsic::abs:
  case Intrinsic::umin:
  case Intrinsic::umax:
  case Intrinsic::smin:
  case Intrinsic::smax:
  case Intrinsic::uadd_sat:
  case Intrinsic::usub_sat:
  case Intrinsic::sadd_sat:
  case Intrinsic::ssub_sat:
  case Intrinsic::fshl:
  case Intrinsic::fshr:
  case Intrinsic::uadd_with_overflow:
  case Intrinsic::usub_with_overflow:
  case Intrinsic::umul_with_overflow:
  case Intrinsic::sadd_with_overflow:
  case Intrinsic::ssub_with_overflow:
  case Intrinsic::smul_with_overflow:
    return false;
  default:
    return true;
  }
}

bool intrinsicScalarizedForInterpreterOracle(Intrinsic::ID ID) {
  switch (ID) {
  case Intrinsic::ctlz:
  case Intrinsic::cttz:
  case Intrinsic::ctpop:
  case Intrinsic::bitreverse:
  case Intrinsic::bswap:
  case Intrinsic::abs:
  case Intrinsic::umin:
  case Intrinsic::umax:
  case Intrinsic::smin:
  case Intrinsic::smax:
  case Intrinsic::uadd_sat:
  case Intrinsic::usub_sat:
  case Intrinsic::sadd_sat:
  case Intrinsic::ssub_sat:
  case Intrinsic::fshl:
  case Intrinsic::fshr:
    return true;
  default:
    return false;
  }
}

bool intrinsicManuallyLoweredForInterpreterOracle(Intrinsic::ID ID) {
  switch (ID) {
  case Intrinsic::bitreverse:
  case Intrinsic::abs:
  case Intrinsic::umin:
  case Intrinsic::umax:
  case Intrinsic::smin:
  case Intrinsic::smax:
  case Intrinsic::uadd_sat:
  case Intrinsic::usub_sat:
  case Intrinsic::sadd_sat:
  case Intrinsic::ssub_sat:
  case Intrinsic::fshl:
  case Intrinsic::fshr:
  case Intrinsic::uadd_with_overflow:
  case Intrinsic::usub_with_overflow:
  case Intrinsic::umul_with_overflow:
  case Intrinsic::sadd_with_overflow:
  case Intrinsic::ssub_with_overflow:
  case Intrinsic::smul_with_overflow:
    return true;
  default:
    return false;
  }
}

ConstantInt *intConst(Type *Ty, uint64_t Value) {
  return ConstantInt::get(cast<IntegerType>(Ty), Value);
}

Type *oracleWideIntegerType(Type *Ty) {
  unsigned Width = cast<IntegerType>(Ty)->getBitWidth();
  unsigned WideWidth = Width < 32 ? 32 : Width * 2;
  WideWidth = std::min(WideWidth, 128u);
  return IntegerType::get(Ty->getContext(), WideWidth);
}

ConstantInt *oracleUnsignedMax(Type *WideTy, unsigned SourceWidth) {
  unsigned WideWidth = cast<IntegerType>(WideTy)->getBitWidth();
  return ConstantInt::get(WideTy->getContext(),
                          APInt::getMaxValue(SourceWidth).zext(WideWidth));
}

ConstantInt *oracleSignedLimit(Type *WideTy, unsigned SourceWidth, bool Min) {
  unsigned WideWidth = cast<IntegerType>(WideTy)->getBitWidth();
  APInt Value = Min ? APInt::getSignedMinValue(SourceWidth)
                    : APInt::getSignedMaxValue(SourceWidth);
  return ConstantInt::get(WideTy->getContext(), Value.sext(WideWidth));
}

Value *notI1(IRBuilder<NoFolder> &B, Value *V, const Twine &Name) {
  return B.CreateXor(V, ConstantInt::getTrue(V->getContext()), Name);
}

Value *lowerOracleBitReverse(IRBuilder<NoFolder> &B, Value *V,
                             const Twine &Name) {
  Type *Ty = V->getType();
  if (!Ty->isIntegerTy())
    return nullptr;
  unsigned Width = Ty->getIntegerBitWidth();
  Value *Result = intConst(Ty, 0);
  for (unsigned I = 0; I != Width; ++I) {
    Value *Bit = B.CreateAnd(B.CreateLShr(V, intConst(Ty, I),
                                          Name + ".shr"),
                             intConst(Ty, 1), Name + ".bit");
    unsigned OutShift = Width - 1 - I;
    if (OutShift != 0)
      Bit = B.CreateShl(Bit, intConst(Ty, OutShift), Name + ".shl");
    Result = B.CreateOr(Result, Bit, Name + ".or");
  }
  return Result;
}

struct SignedOverflowParts {
  Value *Result;
  Value *Positive;
  Value *Negative;
  Value *Any;
};

SignedOverflowParts buildSignedAddSubOverflow(IRBuilder<NoFolder> &B, Value *A,
                                               Value *Bv, bool IsSub,
                                               const Twine &Name) {
  Type *Ty = A->getType();
  Value *Zero = intConst(Ty, 0);
  Value *Result = IsSub ? B.CreateSub(A, Bv, Name + ".diff")
                        : B.CreateAdd(A, Bv, Name + ".sum");
  Value *ANeg = B.CreateICmpSLT(A, Zero, Name + ".a.neg");
  Value *BNeg = B.CreateICmpSLT(Bv, Zero, Name + ".b.neg");
  Value *RNeg = B.CreateICmpSLT(Result, Zero, Name + ".r.neg");
  Value *NotANeg = notI1(B, ANeg, Name + ".a.nonneg");
  Value *NotBNeg = notI1(B, BNeg, Name + ".b.nonneg");
  Value *NotRNeg = notI1(B, RNeg, Name + ".r.nonneg");

  Value *Positive = nullptr;
  Value *Negative = nullptr;
  if (IsSub) {
    Positive = B.CreateAnd(B.CreateAnd(NotANeg, BNeg, Name + ".pos.ab"),
                           RNeg, Name + ".pos");
    Negative = B.CreateAnd(B.CreateAnd(ANeg, NotBNeg, Name + ".neg.ab"),
                           NotRNeg, Name + ".neg");
  } else {
    Positive = B.CreateAnd(B.CreateAnd(NotANeg, NotBNeg, Name + ".pos.ab"),
                           RNeg, Name + ".pos");
    Negative = B.CreateAnd(B.CreateAnd(ANeg, BNeg, Name + ".neg.ab"),
                           NotRNeg, Name + ".neg");
  }
  Value *Any = B.CreateOr(Positive, Negative, Name + ".any");
  return {Result, Positive, Negative, Any};
}

Value *makeOverflowResult(IRBuilder<NoFolder> &B, CallInst *Call,
                          Value *Result, Value *Overflow, const Twine &Name) {
  Value *Pair = PoisonValue::get(Call->getType());
  Pair = B.CreateInsertValue(Pair, Result, {0}, Name + ".value");
  return B.CreateInsertValue(Pair, Overflow, {1}, Name + ".overflow");
}

Value *lowerOracleIntegerIntrinsic(IRBuilder<NoFolder> &B, CallInst *Call) {
  Function *Callee = Call->getCalledFunction();
  if (!Callee || !Callee->isIntrinsic())
    return nullptr;

  Intrinsic::ID ID = Callee->getIntrinsicID();
  Value *A = Call->getArgOperand(0);
  Type *Ty = A->getType();
  bool IsOverflow = isOverflowIntrinsic(ID);
  if (!Ty->isIntegerTy() || (!Call->getType()->isIntegerTy() && !IsOverflow))
    return nullptr;

  switch (ID) {
  case Intrinsic::bitreverse:
    return lowerOracleBitReverse(B, A, "fuzzx.oracle.bitreverse");
  case Intrinsic::abs: {
    Value *Neg = B.CreateSub(intConst(Ty, 0), A, "fuzzx.oracle.abs.neg");
    Value *IsNeg =
        B.CreateICmpSLT(A, intConst(Ty, 0), "fuzzx.oracle.abs.isneg");
    return B.CreateSelect(IsNeg, Neg, A, "fuzzx.oracle.abs");
  }
  case Intrinsic::umin:
  case Intrinsic::umax:
  case Intrinsic::smin:
  case Intrinsic::smax: {
    Value *Bv = Call->getArgOperand(1);
    ICmpInst::Predicate Pred;
    if (ID == Intrinsic::umin)
      Pred = ICmpInst::ICMP_ULT;
    else if (ID == Intrinsic::umax)
      Pred = ICmpInst::ICMP_UGT;
    else if (ID == Intrinsic::smin)
      Pred = ICmpInst::ICMP_SLT;
    else
      Pred = ICmpInst::ICMP_SGT;
    Value *Cmp = B.CreateICmp(Pred, A, Bv, "fuzzx.oracle.minmax.cmp");
    return B.CreateSelect(Cmp, A, Bv, "fuzzx.oracle.minmax");
  }
  case Intrinsic::uadd_sat: {
    Value *Bv = Call->getArgOperand(1);
    Type *WideTy = oracleWideIntegerType(Ty);
    unsigned Width = cast<IntegerType>(Ty)->getBitWidth();
    Value *AWide = B.CreateZExt(A, WideTy, "fuzzx.oracle.uadd.sat.a");
    Value *BWide = B.CreateZExt(Bv, WideTy, "fuzzx.oracle.uadd.sat.b");
    Value *Sum = B.CreateAdd(AWide, BWide, "fuzzx.oracle.uadd.sat.sum");
    ConstantInt *Max = oracleUnsignedMax(WideTy, Width);
    Value *Overflow = B.CreateICmpUGT(Sum, Max, "fuzzx.oracle.uadd.sat.ov");
    Value *Clamped =
        B.CreateSelect(Overflow, Max, Sum, "fuzzx.oracle.uadd.sat.clamp");
    return B.CreateTrunc(Clamped, Ty, "fuzzx.oracle.uadd.sat");
  }
  case Intrinsic::usub_sat: {
    Value *Bv = Call->getArgOperand(1);
    Type *WideTy = oracleWideIntegerType(Ty);
    Value *AWide = B.CreateZExt(A, WideTy, "fuzzx.oracle.usub.sat.a");
    Value *BWide = B.CreateZExt(Bv, WideTy, "fuzzx.oracle.usub.sat.b");
    Value *Diff = B.CreateSub(AWide, BWide, "fuzzx.oracle.usub.sat.diff");
    Value *Overflow = B.CreateICmpULT(AWide, BWide,
                                      "fuzzx.oracle.usub.sat.ov");
    Value *Clamped =
        B.CreateSelect(Overflow, ConstantInt::get(WideTy, 0), Diff,
                       "fuzzx.oracle.usub.sat.clamp");
    return B.CreateTrunc(Clamped, Ty, "fuzzx.oracle.usub.sat");
  }
  case Intrinsic::sadd_sat:
  case Intrinsic::ssub_sat: {
    Value *Bv = Call->getArgOperand(1);
    Type *WideTy = oracleWideIntegerType(Ty);
    unsigned Width = cast<IntegerType>(Ty)->getBitWidth();
    Value *AWide = B.CreateSExt(A, WideTy, "fuzzx.oracle.ssat.a");
    Value *BWide = B.CreateSExt(Bv, WideTy, "fuzzx.oracle.ssat.b");
    Value *Raw = ID == Intrinsic::ssub_sat
                     ? B.CreateSub(AWide, BWide, "fuzzx.oracle.ssat.raw")
                     : B.CreateAdd(AWide, BWide, "fuzzx.oracle.ssat.raw");
    ConstantInt *Min = oracleSignedLimit(WideTy, Width, true);
    ConstantInt *Max = oracleSignedLimit(WideTy, Width, false);
    Value *TooHigh = B.CreateICmpSGT(Raw, Max, "fuzzx.oracle.ssat.high");
    Value *HighClamped =
        B.CreateSelect(TooHigh, Max, Raw, "fuzzx.oracle.ssat.high.clamp");
    Value *TooLow =
        B.CreateICmpSLT(HighClamped, Min, "fuzzx.oracle.ssat.low");
    Value *Clamped =
        B.CreateSelect(TooLow, Min, HighClamped,
                       "fuzzx.oracle.ssat.low.clamp");
    return B.CreateTrunc(Clamped, Ty, "fuzzx.oracle.ssat");
  }
  case Intrinsic::fshl:
  case Intrinsic::fshr: {
    Value *Bv = Call->getArgOperand(1);
    Value *Amt = Call->getArgOperand(2);
    unsigned Width = Ty->getIntegerBitWidth();
    Value *Shift = B.CreateAnd(Amt, intConst(Ty, Width - 1),
                               "fuzzx.oracle.fsh.shift");
    Value *InvShift =
        B.CreateAnd(B.CreateSub(intConst(Ty, Width), Shift,
                                "fuzzx.oracle.fsh.inv.raw"),
                    intConst(Ty, Width - 1), "fuzzx.oracle.fsh.inv");
    Value *IsZero =
        B.CreateICmpEQ(Shift, intConst(Ty, 0), "fuzzx.oracle.fsh.zero");
    if (ID == Intrinsic::fshl) {
      Value *Left = B.CreateShl(A, Shift, "fuzzx.oracle.fshl.left");
      Value *Right = B.CreateLShr(Bv, InvShift, "fuzzx.oracle.fshl.right");
      Value *Combined = B.CreateOr(Left, Right, "fuzzx.oracle.fshl.or");
      return B.CreateSelect(IsZero, A, Combined, "fuzzx.oracle.fshl");
    }
    Value *Left = B.CreateShl(A, InvShift, "fuzzx.oracle.fshr.left");
    Value *Right = B.CreateLShr(Bv, Shift, "fuzzx.oracle.fshr.right");
    Value *Combined = B.CreateOr(Left, Right, "fuzzx.oracle.fshr.or");
    return B.CreateSelect(IsZero, Bv, Combined, "fuzzx.oracle.fshr");
  }
  case Intrinsic::uadd_with_overflow: {
    Value *Bv = Call->getArgOperand(1);
    Value *Sum = B.CreateAdd(A, Bv, "fuzzx.oracle.uadd.ov.sum");
    Value *Overflow = B.CreateICmpULT(Sum, A, "fuzzx.oracle.uadd.ov");
    return makeOverflowResult(B, Call, Sum, Overflow, "fuzzx.oracle.uadd.ov");
  }
  case Intrinsic::usub_with_overflow: {
    Value *Bv = Call->getArgOperand(1);
    Value *Diff = B.CreateSub(A, Bv, "fuzzx.oracle.usub.ov.diff");
    Value *Overflow = B.CreateICmpULT(A, Bv, "fuzzx.oracle.usub.ov");
    return makeOverflowResult(B, Call, Diff, Overflow, "fuzzx.oracle.usub.ov");
  }
  case Intrinsic::umul_with_overflow: {
    Value *Bv = Call->getArgOperand(1);
    unsigned Width = Ty->getIntegerBitWidth();
    Type *WideTy = IntegerType::get(Ty->getContext(), Width * 2);
    Value *WideA = B.CreateZExt(A, WideTy, "fuzzx.oracle.umul.ov.a");
    Value *WideB = B.CreateZExt(Bv, WideTy, "fuzzx.oracle.umul.ov.b");
    Value *WideProduct =
        B.CreateMul(WideA, WideB, "fuzzx.oracle.umul.ov.wide");
    Value *Product = B.CreateTrunc(WideProduct, Ty,
                                   "fuzzx.oracle.umul.ov.product");
    Value *High = B.CreateLShr(WideProduct, ConstantInt::get(WideTy, Width),
                               "fuzzx.oracle.umul.ov.high");
    Value *Overflow =
        B.CreateICmpNE(High, ConstantInt::get(WideTy, 0),
                       "fuzzx.oracle.umul.ov");
    return makeOverflowResult(B, Call, Product, Overflow,
                              "fuzzx.oracle.umul.ov");
  }
  case Intrinsic::sadd_with_overflow:
  case Intrinsic::ssub_with_overflow: {
    Value *Bv = Call->getArgOperand(1);
    SignedOverflowParts Parts = buildSignedAddSubOverflow(
        B, A, Bv, ID == Intrinsic::ssub_with_overflow, "fuzzx.oracle.sov");
    return makeOverflowResult(B, Call, Parts.Result, Parts.Any,
                              "fuzzx.oracle.sov");
  }
  case Intrinsic::smul_with_overflow: {
    Value *Bv = Call->getArgOperand(1);
    unsigned Width = Ty->getIntegerBitWidth();
    Type *WideTy = IntegerType::get(Ty->getContext(), Width * 2);
    Value *WideA = B.CreateSExt(A, WideTy, "fuzzx.oracle.smul.ov.a");
    Value *WideB = B.CreateSExt(Bv, WideTy, "fuzzx.oracle.smul.ov.b");
    Value *WideProduct =
        B.CreateMul(WideA, WideB, "fuzzx.oracle.smul.ov.wide");
    Value *Product = B.CreateTrunc(WideProduct, Ty,
                                   "fuzzx.oracle.smul.ov.product");
    Value *SignExtended =
        B.CreateSExt(Product, WideTy, "fuzzx.oracle.smul.ov.sext");
    Value *Overflow = B.CreateICmpNE(WideProduct, SignExtended,
                                     "fuzzx.oracle.smul.ov");
    return makeOverflowResult(B, Call, Product, Overflow,
                              "fuzzx.oracle.smul.ov");
  }
  default:
    return nullptr;
  }
}

bool moduleSupportedByInterpreterOracle(const Module &M) {
  const Function *Kernel = nullptr;
  for (const Function &F : M) {
    if ((F.getName() == "fuzz_kernel" || F.getName() == "fuzz_kernel_o0" ||
         F.getName() == "fuzz_kernel_o2") &&
        !F.isDeclaration() && hasIRKernelSignature(F)) {
      Kernel = &F;
      break;
    }
  }
  if (!Kernel)
    return false;

  for (const BasicBlock &BB : *Kernel) {
    for (const Instruction &I : BB) {
      if (typeUnsupportedByInterpreterOracle(I.getType()))
        return false;
      for (const Value *Op : I.operands())
        if (typeUnsupportedByInterpreterOracle(Op->getType()))
          return false;
      if (const auto *Call = dyn_cast<CallInst>(&I)) {
        const Function *Callee = Call->getCalledFunction();
        if (!Callee || !Callee->isIntrinsic())
          return false;
        if (intrinsicUnsupportedByInterpreterOracle(Callee->getIntrinsicID()))
          return false;
      }
    }
  }
  return true;
}

bool scalarizeOracleVectorIntrinsics(Module &M) {
  SmallVector<CallInst *, 16> ToScalarize;
  for (Function &F : M) {
    if (F.isDeclaration())
      continue;
    for (BasicBlock &BB : F) {
      for (Instruction &I : BB) {
        auto *Call = dyn_cast<CallInst>(&I);
        if (!Call)
          continue;
        Function *Callee = Call->getCalledFunction();
        if (!Callee || !Callee->isIntrinsic() ||
            !intrinsicScalarizedForInterpreterOracle(Callee->getIntrinsicID()))
          continue;
        auto *VecTy = dyn_cast<FixedVectorType>(Call->getType());
        if (VecTy)
          ToScalarize.push_back(Call);
      }
    }
  }

  for (CallInst *Call : ToScalarize) {
    Function *Callee = Call->getCalledFunction();
    Intrinsic::ID ID = Callee->getIntrinsicID();
    auto *VecTy = cast<FixedVectorType>(Call->getType());
    Type *EltTy = VecTy->getElementType();
    Function *ScalarDecl = Intrinsic::getOrInsertDeclaration(&M, ID, {EltTy});
    IRBuilder<NoFolder> B(Call);
    Value *Result = PoisonValue::get(VecTy);

    for (unsigned I = 0, E = VecTy->getNumElements(); I != E; ++I) {
      Value *Lane = ci32(M.getContext(), I);
      SmallVector<Value *, 2> Args;
      for (unsigned ArgNo = 0; ArgNo != Call->arg_size(); ++ArgNo) {
        Value *Arg = Call->getArgOperand(ArgNo);
        if (isa<FixedVectorType>(Arg->getType())) {
          Args.push_back(B.CreateExtractElement(
              Arg, Lane, "fuzzx.oracle.scalar.arg"));
        } else {
          Args.push_back(Arg);
        }
      }
      Value *Scalar =
          B.CreateCall(ScalarDecl, Args, "fuzzx.oracle.scalar.intr");
      Result = B.CreateInsertElement(Result, Scalar, Lane,
                                     "fuzzx.oracle.scalarized");
    }

    Call->replaceAllUsesWith(Result);
    Call->eraseFromParent();
  }

  return true;
}

bool lowerOracleIntegerIntrinsics(Module &M) {
  SmallVector<CallInst *, 32> ToLower;
  for (Function &F : M) {
    if (F.isDeclaration())
      continue;
    for (BasicBlock &BB : F) {
      for (Instruction &I : BB) {
        auto *Call = dyn_cast<CallInst>(&I);
        if (!Call)
          continue;
        Function *Callee = Call->getCalledFunction();
        if (!Callee || !Callee->isIntrinsic() ||
            !intrinsicManuallyLoweredForInterpreterOracle(
                Callee->getIntrinsicID()))
          continue;
        ToLower.push_back(Call);
      }
    }
  }

  for (CallInst *Call : ToLower) {
    IRBuilder<NoFolder> B(Call);
    Value *Replacement = lowerOracleIntegerIntrinsic(B, Call);
    if (!Replacement)
      return false;
    Call->replaceAllUsesWith(Replacement);
    Call->eraseFromParent();
  }
  return true;
}

bool lowerOracleThreadIntrinsics(Module &M, GlobalVariable *OracleWI) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  SmallVector<Instruction *, 8> ToErase;

  for (Function &F : M) {
    if (F.isDeclaration())
      continue;
    for (BasicBlock &BB : F) {
      for (Instruction &I : BB) {
        auto *Call = dyn_cast<CallInst>(&I);
        if (!Call)
          continue;
        Function *Callee = Call->getCalledFunction();
        if (!Callee || !Callee->isIntrinsic())
          continue;

        Intrinsic::ID ID = Callee->getIntrinsicID();
        if (ID == Intrinsic::amdgcn_workgroup_id_x) {
          Call->replaceAllUsesWith(ci32(Ctx, 0));
          ToErase.push_back(Call);
        } else if (ID == Intrinsic::amdgcn_workitem_id_x) {
          IRBuilder<NoFolder> B(Call);
          Value *WI = B.CreateLoad(I32, OracleWI, "fuzzx.oracle.wi.load");
          Call->replaceAllUsesWith(WI);
          ToErase.push_back(Call);
        }
      }
    }
  }

  for (Instruction *I : ToErase)
    I->eraseFromParent();
  return true;
}

std::optional<std::array<uint32_t, InputCount>>
computeInterpreterOracleOutputs(const Module &Input,
                                ArrayRef<uint32_t> Inputs) {
  if (!moduleSupportedByInterpreterOracle(Input))
    return std::nullopt;

  std::unique_ptr<Module> OracleM = CloneModule(Input);
  Function *Kernel = findIRKernel(*OracleM);
  if (!Kernel)
    return std::nullopt;

  LLVMContext &Ctx = OracleM->getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  auto *OracleWI = new GlobalVariable(*OracleM, I32, false,
                                      GlobalValue::InternalLinkage, ci32(Ctx, 0),
                                      "fuzzx.oracle.wi");
  if (!scalarizeOracleVectorIntrinsics(*OracleM))
    return std::nullopt;
  if (!lowerOracleIntegerIntrinsics(*OracleM))
    return std::nullopt;
  lowerOracleThreadIntrinsics(*OracleM, OracleWI);
  Kernel->setCallingConv(CallingConv::C);
  Kernel->setVisibility(GlobalValue::DefaultVisibility);

  std::string Error;
  std::unique_ptr<ExecutionEngine> EE(
      EngineBuilder(std::move(OracleM))
          .setEngineKind(EngineKind::Interpreter)
          .setErrorStr(&Error)
          .create());
  if (!EE)
    return std::nullopt;

  auto *LanePtr = reinterpret_cast<uint32_t *>(EE->getPointerToGlobal(OracleWI));
  if (!LanePtr)
    return std::nullopt;

  std::array<uint32_t, InputCount> Outputs{};
  std::vector<GenericValue> Args(3);
  Args[0].PointerVal = const_cast<uint32_t *>(Inputs.data());
  Args[1].PointerVal = Outputs.data();
  Args[2].IntVal = APInt(32, Inputs.size());

  for (unsigned I = 0; I < Inputs.size(); ++I) {
    *LanePtr = I;
    bool Ran = false;
    CrashRecoveryContext CRC;
    if (!CRC.RunSafely([&] {
          EE->runFunction(Kernel, Args);
          Ran = true;
        }))
      return std::nullopt;
    if (!Ran)
      return std::nullopt;
  }

  return Outputs;
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

  std::ofstream Raw(Dir / "fuzzer-input.bc", std::ios::binary);
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

void saveFailureFinding(const uint8_t *Data, size_t Size, StringRef IR,
                        StringRef Kind, StringRef Stage,
                        std::optional<int> CrashRetCode = std::nullopt) {
  const char *RootEnv = std::getenv("FUZZX_FINDINGS_DIR");
  std::filesystem::path Root = RootEnv && *RootEnv ? RootEnv : "findings";
  std::filesystem::create_directories(Root);
  auto Dir = Root / ("cxx-failure-" + std::to_string(std::time(nullptr)) +
                     "-" + std::to_string(getpid()));
  std::filesystem::create_directories(Dir);

  std::ofstream Raw(Dir / "fuzzer-input.bc", std::ios::binary);
  Raw.write(reinterpret_cast<const char *>(Data),
            static_cast<std::streamsize>(Size));
  std::ofstream LL(Dir / "program.ll");
  LL << IR.str();
  std::ofstream Failure(Dir / "failure.txt");
  Failure << "kind=" << Kind.str() << "\n"
          << "stage=" << Stage.str() << "\n";
  if (CrashRetCode)
    Failure << "crash_retcode=" << *CrashRetCode << "\n";
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

bool useInterpreterOracle() {
  return envFlag("FUZZX_USE_LLVM_INTERPRETER_ORACLE", false) ||
         envFlag("FUZZX_REQUIRE_LLVM_INTERPRETER_ORACLE", false);
}

bool requireInterpreterOracle() {
  return envFlag("FUZZX_REQUIRE_LLVM_INTERPRETER_ORACLE", false);
}

} // namespace

extern "C" size_t LLVMFuzzerCustomMutator(uint8_t *Data, size_t Size,
                                          size_t MaxSize, unsigned Seed) {
  StringRef CPU = getCPU();
  LLVMContext Ctx;
  std::unique_ptr<Module> M = parseIRCorpusModule(Data, Size, Ctx, CPU);
  std::minstd_rand Gen(Seed);
  mutateIRModule(*M, Gen);
  if (!validateIRCorpusModule(*M))
    return 0;
  if (requireInterpreterOracle() && !moduleSupportedByInterpreterOracle(*M))
    return 0;

  std::vector<uint8_t> Out = moduleToBitcode(*M);
  if (Out.empty() || Out.size() > MaxSize)
    return 0;
  std::memcpy(Data, Out.data(), Out.size());
  return Out.size();
}

extern "C" size_t LLVMFuzzerCustomCrossOver(const uint8_t *Data1,
                                            size_t Size1,
                                            const uint8_t *Data2,
                                            size_t Size2, uint8_t *Out,
                                            size_t MaxOutSize,
                                            unsigned Seed) {
  StringRef CPU = getCPU();
  LLVMContext Ctx;
  std::unique_ptr<Module> Base = parseIRCorpusModule(Data1, Size1, Ctx, CPU);
  std::unique_ptr<Module> Other = parseIRCorpusModule(Data2, Size2, Ctx, CPU);
  std::minstd_rand Gen(Seed);
  if (!crossOverIRModules(*Base, *Other, Gen))
    mutateIRModule(*Base, Gen);
  if (!validateIRCorpusModule(*Base))
    return 0;
  if (requireInterpreterOracle() && !moduleSupportedByInterpreterOracle(*Base))
    return 0;
  std::vector<uint8_t> Buffer = moduleToBitcode(*Base);
  if (Buffer.empty() || Buffer.size() > MaxOutSize)
    return 0;
  std::memcpy(Out, Buffer.data(), Buffer.size());
  return Buffer.size();
}

extern "C" int LLVMFuzzerTestOneInput(const uint8_t *Data, size_t Size) {
  if (Size > 1 << 20)
    return 0;

  StringRef CPU = getCPU();
  LLVMContext Ctx;
  bool ValidInput = false;
  std::unique_ptr<Module> M =
      parseIRCorpusModule(Data, Size, Ctx, CPU, &ValidInput);
  if (!ValidInput)
    return 0;
  if (!validateIRCorpusModule(*M))
    return 0;
  auto Inputs = makeInputs(Data, Size);
  std::optional<std::array<uint32_t, InputCount>> ExpectedOutputs;
  if (useInterpreterOracle())
    ExpectedOutputs = computeInterpreterOracleOutputs(*M, Inputs);
  if (requireInterpreterOracle() && !ExpectedOutputs)
    return 0;

  std::string O0IR;
  auto O0Obj =
      compileIRModuleToObject(*M, CPU, OptimizationLevel::O0, "fuzz_kernel_o0",
                              &O0IR);
  if (!O0Obj.Success) {
    if (O0Obj.FailureStage != "validate") {
      std::string Stage = "o0-" + O0Obj.FailureStage;
      saveFailureFinding(Data, Size, O0IR,
                         O0Obj.Crashed ? "compiler-crash"
                                       : "compiler-failure",
                         Stage,
                         O0Obj.Crashed
                             ? std::optional<int>(O0Obj.CrashRetCode)
                             : std::nullopt);
      std::abort();
    }
    return 0;
  }
  std::string O2IR;
  auto O2Obj =
      compileIRModuleToObject(*M, CPU, OptimizationLevel::O2, "fuzz_kernel_o2",
                              &O2IR);
  if (!O2Obj.Success) {
    if (O2Obj.FailureStage != "validate") {
      std::string Stage = "o2-" + O2Obj.FailureStage;
      saveFailureFinding(Data, Size, O2IR,
                         O2Obj.Crashed ? "compiler-crash"
                                       : "compiler-failure",
                         Stage,
                         O2Obj.Crashed
                             ? std::optional<int>(O2Obj.CrashRetCode)
                             : std::nullopt);
      std::abort();
    }
    return 0;
  }
  auto HsacoPath = linkObjectsToHsaco(O0Obj.Object, O2Obj.Object);
  if (!HsacoPath) {
    saveFailureFinding(Data, Size, O0IR, "link-failure", "hsaco-link");
    std::abort();
    return 0;
  }

  std::array<uint32_t, InputCount> O0Outputs{};
  std::array<uint32_t, InputCount> O2Outputs{};
  bool Ran =
      runBothOnGpu(*HsacoPath, Inputs, MutableArrayRef<uint32_t>(O0Outputs),
                   MutableArrayRef<uint32_t>(O2Outputs));
  if (!Ran) {
    std::filesystem::remove(*HsacoPath);
    return 0;
  }

  for (unsigned I = 0; I < InputCount; ++I) {
    std::optional<uint32_t> Expected;
    bool O0O2Mismatch = O0Outputs[I] != O2Outputs[I];
    bool OracleMismatch = false;
    if (ExpectedOutputs) {
      Expected = (*ExpectedOutputs)[I];
      OracleMismatch = O0Outputs[I] != *Expected || O2Outputs[I] != *Expected;
    }
    if (O0O2Mismatch || OracleMismatch) {
      StringRef Kind = OracleMismatch ? "oracle" : "differential";
      saveFinding(Data, Size, O0IR, *HsacoPath, Kind, I, Inputs[I],
                  O0Outputs[I], O2Outputs[I], Expected);
      std::abort();
    }
  }
  std::filesystem::remove(*HsacoPath);
  return 0;
}

extern "C" int LLVMFuzzerInitialize(int *, char ***) {
  CrashRecoveryContext::Enable();
  if (hipSetDevice(getDevice()) != hipSuccess)
    return 1;
  return 0;
}
