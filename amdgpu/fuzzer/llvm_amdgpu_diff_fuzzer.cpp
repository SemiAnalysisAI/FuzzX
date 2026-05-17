#include "lld/Common/Driver.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/ADT/StringExtras.h"
#include "llvm/Bitcode/BitcodeReader.h"
#include "llvm/Bitcode/BitcodeWriter.h"
#include "llvm/IR/Constants.h"
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
  case Intrinsic::fshl:
  case Intrinsic::fshr:
    return true;
  default:
    return false;
  }
}

bool isKnownNonZeroI32(const Value *V) {
  if (const auto *C = dyn_cast<ConstantInt>(V))
    return C->getType()->isIntegerTy(32) && !C->isZero();
  const auto *BO = dyn_cast<BinaryOperator>(V);
  if (!BO || BO->getOpcode() != Instruction::Or ||
      !BO->getType()->isIntegerTy(32))
    return false;
  if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(0)))
    return !C->isZero();
  if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(1)))
    return !C->isZero();
  return false;
}

bool isAllowedIRInstruction(const Instruction &I) {
  if (isa<BranchInst, ReturnInst, LoadInst, StoreInst, GetElementPtrInst,
          ZExtInst, SExtInst, TruncInst, ICmpInst, SelectInst>(&I))
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
    case Instruction::UDiv:
    case Instruction::URem:
      return isKnownNonZeroI32(BO->getOperand(1));
    case Instruction::Shl:
    case Instruction::LShr:
    case Instruction::AShr:
      if (auto *Shift = dyn_cast<ConstantInt>(BO->getOperand(1)))
        return Shift->getValue().ult(BO->getType()->getIntegerBitWidth());
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

bool isHighBitI32Constant(const Value *V) {
  const auto *CI = dyn_cast<ConstantInt>(V);
  return CI && CI->getType()->isIntegerTy(32) &&
         (CI->getZExtValue() & 0x80000000ull) != 0;
}

bool isOrWithHighBitConstantOf(const Value *MaybeOr, const Value *Other) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeOr);
  if (!BO || BO->getOpcode() != Instruction::Or)
    return false;
  return (BO->getOperand(0) == Other &&
          isHighBitI32Constant(BO->getOperand(1))) ||
         (BO->getOperand(1) == Other &&
          isHighBitI32Constant(BO->getOperand(0)));
}

bool triggersM019HighBitOrXor(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Xor ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isOrWithHighBitConstantOf(BO->getOperand(0), BO->getOperand(1)) ||
         isOrWithHighBitConstantOf(BO->getOperand(1), BO->getOperand(0));
}

bool isOrWithOperand(const Value *MaybeOr, const Value *Operand) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeOr);
  if (!BO || BO->getOpcode() != Instruction::Or)
    return false;
  return BO->getOperand(0) == Operand || BO->getOperand(1) == Operand;
}

bool isXorOfOrWithOrOperand(const Value *MaybeXor, const Value *MaybeOr) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeXor);
  if (!BO || BO->getOpcode() != Instruction::Xor)
    return false;
  if (BO->getOperand(0) == MaybeOr)
    return isOrWithOperand(MaybeOr, BO->getOperand(1));
  if (BO->getOperand(1) == MaybeOr)
    return isOrWithOperand(MaybeOr, BO->getOperand(0));
  return false;
}

bool triggersM020OrXorAnd(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isXorOfOrWithOrOperand(BO->getOperand(0), BO->getOperand(1)) ||
         isXorOfOrWithOrOperand(BO->getOperand(1), BO->getOperand(0));
}

bool feedsM020OrXorAnd(const Instruction &I) {
  for (const User *U : I.users())
    if (const auto *UserI = dyn_cast<Instruction>(U))
      if (triggersM020OrXorAnd(*UserI))
        return true;
  return false;
}

bool triggersM021OrXor(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Xor ||
      !BO->getType()->isIntegerTy(32))
    return false;
  if (triggersM019HighBitOrXor(I) || feedsM020OrXorAnd(I))
    return false;
  return isOrWithOperand(BO->getOperand(0), BO->getOperand(1)) ||
         isOrWithOperand(BO->getOperand(1), BO->getOperand(0));
}

const Value *stripI32AndAllOnes(const Value *V) {
  while (const auto *BO = dyn_cast<BinaryOperator>(V)) {
    if (BO->getOpcode() != Instruction::And || !BO->getType()->isIntegerTy(32))
      return V;
    if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(0))) {
      if (C->isMinusOne()) {
        V = BO->getOperand(1);
        continue;
      }
    }
    if (const auto *C = dyn_cast<ConstantInt>(BO->getOperand(1))) {
      if (C->isMinusOne()) {
        V = BO->getOperand(0);
        continue;
      }
    }
    return V;
  }
  return V;
}

bool isXorWithConstantOf(const Value *MaybeXor, const Value *Operand) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeXor);
  if (!BO || BO->getOpcode() != Instruction::Xor)
    return false;
  Operand = stripI32AndAllOnes(Operand);
  return (stripI32AndAllOnes(BO->getOperand(0)) == Operand &&
          isa<ConstantInt>(BO->getOperand(1))) ||
         (stripI32AndAllOnes(BO->getOperand(1)) == Operand &&
          isa<ConstantInt>(BO->getOperand(0)));
}

bool triggersM022AndXorConstant(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::And ||
      !BO->getType()->isIntegerTy(32))
    return false;
  const Value *LHS = stripI32AndAllOnes(BO->getOperand(0));
  const Value *RHS = stripI32AndAllOnes(BO->getOperand(1));
  return isXorWithConstantOf(LHS, RHS) || isXorWithConstantOf(RHS, LHS);
}

bool isAndWithOperand(const Value *MaybeAnd, const Value *Operand) {
  const auto *BO = dyn_cast<BinaryOperator>(MaybeAnd);
  if (!BO || BO->getOpcode() != Instruction::And)
    return false;
  return BO->getOperand(0) == Operand || BO->getOperand(1) == Operand;
}

bool triggersM023AndXorIdentity(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  if (!BO || BO->getOpcode() != Instruction::Xor ||
      !BO->getType()->isIntegerTy(32))
    return false;
  return isAndWithOperand(BO->getOperand(0), BO->getOperand(1)) ||
         isAndWithOperand(BO->getOperand(1), BO->getOperand(0));
}

bool hasName(const Value *V, StringRef Name) {
  return V && V->hasName() && V->getName() == Name;
}

bool validateFixedMemoryShape(Function &Kernel) {
  unsigned GEPs = 0;
  unsigned Loads = 0;
  unsigned Stores = 0;
  for (BasicBlock &BB : Kernel) {
    for (Instruction &I : BB) {
      if (auto *GEP = dyn_cast<GetElementPtrInst>(&I)) {
        ++GEPs;
        if (GEP->getSourceElementType() != Type::getInt32Ty(Kernel.getContext()))
          return false;
        if (!hasName(GEP, "in.ptr") && !hasName(GEP, "out.ptr"))
          return false;
        if (GEP->getNumIndices() != 1 ||
            !hasName(GEP->idx_begin()->get(), "idx64"))
          return false;
        unsigned ArgNo = hasName(GEP, "in.ptr") ? 0 : 1;
        if (GEP->getPointerOperand() != Kernel.getArg(ArgNo))
          return false;
      } else if (auto *Load = dyn_cast<LoadInst>(&I)) {
        ++Loads;
        if (!Load->getType()->isIntegerTy(32) ||
            !hasName(Load->getPointerOperand(), "in.ptr"))
          return false;
      } else if (auto *Store = dyn_cast<StoreInst>(&I)) {
        ++Stores;
        if (!Store->getValueOperand()->getType()->isIntegerTy(32) ||
            !hasName(Store->getPointerOperand(), "out.ptr"))
          return false;
      }
    }
  }
  return GEPs == 2 && Loads == 1 && Stores == 1;
}

bool validateIRCorpusModule(Module &M) {
  bool AllowM019 = envFlag("FUZZX_ALLOW_M019_HIGHBIT_OR_XOR", false);
  bool AllowM020 = envFlag("FUZZX_ALLOW_M020_OR_XOR_AND", false);
  bool AllowM021 = envFlag("FUZZX_ALLOW_M021_OR_XOR", false);
  bool AllowM022 = envFlag("FUZZX_ALLOW_M022_AND_XOR_CONSTANT", false);
  bool AllowM023 = envFlag("FUZZX_ALLOW_M023_AND_XOR_IDENTITY", false);
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
      if (!validateFixedMemoryShape(F))
        return false;
      for (BasicBlock &BB : F)
        for (Instruction &I : BB)
          if (!isAllowedIRInstruction(I) ||
              (!AllowM019 && triggersM019HighBitOrXor(I)) ||
              (!AllowM020 && triggersM020OrXorAnd(I)) ||
              (!AllowM021 && triggersM021OrXor(I)) ||
              (!AllowM022 && triggersM022AndXorConstant(I)) ||
              (!AllowM023 && triggersM023AndXorIdentity(I)))
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
  for (BasicBlock &BB : *F) {
    for (Instruction &I : BB) {
      if (&I == InsertPt)
        return Values;
      if (I.getType() == I32)
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

Value *emitRandomIRInstruction(IRBuilder<NoFolder> &B, Module &M,
                               Instruction *InsertPt, Value *Current,
                               std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *A = Current;
  Value *Bv = chooseI32Value(InsertPt, Gen);
  switch (Gen() % 34) {
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
  default:
    return B.CreateCall(
        Intrinsic::getOrInsertDeclaration(&M, Intrinsic::fshr, {I32}),
        {A, Bv, chooseI32Value(InsertPt, Gen)}, "fuzz.fshr.dyn");
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

void mutateIRModifyConstant(Module &M, std::minstd_rand &Gen) {
  Function *F = findIRKernel(M);
  if (!F)
    return;
  SmallVector<std::pair<Instruction *, unsigned>, 32> Candidates;
  for (BasicBlock &BB : *F) {
    for (Instruction &I : BB) {
      if (!I.getName().starts_with("fuzz."))
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
    I->setOperand(IOp, ConstantInt::get(Ty, Gen() & 31u));
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
  while (NumMutations < 16 && (Gen() % 4) == 0)
    ++NumMutations;
  for (unsigned I = 0; I < NumMutations; ++I) {
    switch (Gen() % 5) {
    case 0:
    case 1:
    case 2:
      mutateIRAddInstruction(M, Gen);
      break;
    case 3:
      mutateIRModifyConstant(M, Gen);
      break;
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

std::optional<SmallVector<char, 0>>
compileIRModuleToObject(const Module &Input, StringRef CPU,
                        OptimizationLevel Level, StringRef KernelName,
                        std::string *IR = nullptr) {
  TargetMachine *TM = getTargetMachine(CPU, Level);
  std::unique_ptr<Module> M = CloneModule(Input);
  prepareIRModuleForCompile(*M, CPU, KernelName);
  M->setDataLayout(TM->createDataLayout());
  if (IR)
    *IR = moduleToString(*M);
  if (!validateIRCorpusModule(*M))
    return std::nullopt;
  if (!runOptimizationPipeline(*M, *TM, Level))
    return std::nullopt;
  return emitObject(*M, *TM);
}

std::vector<uint8_t> moduleToBitcode(Module &M) {
  SmallVector<char, 0> Buffer;
  raw_svector_ostream OS(Buffer);
  WriteBitcodeToFile(M, OS);
  return std::vector<uint8_t>(Buffer.begin(), Buffer.end());
}

bool isCrossOverCloneableInstruction(const Instruction &I) {
  if (I.isTerminator() || I.mayReadOrWriteMemory() || isa<GetElementPtrInst>(&I))
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

StringRef getCPU() {
  const char *CPU = std::getenv("AMDGPU_MCPU");
  return CPU && *CPU ? StringRef(CPU) : StringRef("gfx950");
}

int getDevice() {
  const char *Device = std::getenv("HIP_DEVICE");
  return Device && *Device ? std::atoi(Device) : 0;
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

  std::string IR;
  auto O0Obj =
      compileIRModuleToObject(*M, CPU, OptimizationLevel::O0, "fuzz_kernel_o0",
                              &IR);
  if (!O0Obj)
    return 0;
  auto O2Obj =
      compileIRModuleToObject(*M, CPU, OptimizationLevel::O2, "fuzz_kernel_o2");
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

  for (unsigned I = 0; I < InputCount; ++I) {
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
