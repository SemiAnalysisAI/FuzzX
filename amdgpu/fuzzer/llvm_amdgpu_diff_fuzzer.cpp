#include "lld/Common/Driver.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/ADT/StringExtras.h"
#include "llvm/Bitcode/BitcodeReader.h"
#include "llvm/Bitcode/BitcodeWriter.h"
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
constexpr unsigned MaxIRCFGBlocks = 320;

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

bool isValidVectorLaneIndex(Type *VecTy, const Value *Index) {
  auto *VT = dyn_cast<FixedVectorType>(VecTy);
  auto *C = dyn_cast<ConstantInt>(Index);
  return VT && C && C->getZExtValue() < VT->getNumElements();
}

bool isValidVectorInstruction(const Instruction &I) {
  if (const auto *Insert = dyn_cast<InsertElementInst>(&I)) {
    return isFixedIntVectorType(Insert->getType(), 32) &&
           Insert->getOperand(1)->getType()->isIntegerTy(32) &&
           isValidVectorLaneIndex(Insert->getType(), Insert->getOperand(2));
  }
  if (const auto *Extract = dyn_cast<ExtractElementInst>(&I)) {
    return Extract->getType()->isIntegerTy(32) &&
           isFixedIntVectorType(Extract->getVectorOperandType(), 32) &&
           isValidVectorLaneIndex(Extract->getVectorOperandType(),
                                  Extract->getIndexOperand());
  }
  if (const auto *BO = dyn_cast<BinaryOperator>(&I)) {
    if (!BO->getType()->isVectorTy())
      return true;
    if (!isFixedIntVectorType(BO->getType(), 32))
      return false;
    if (BO->isShift())
      return isConstantIntOrVectorBelow(BO->getOperand(1), 32);
  }
  if (const auto *Cmp = dyn_cast<ICmpInst>(&I)) {
    if (!Cmp->getOperand(0)->getType()->isVectorTy())
      return true;
    return isFixedIntVectorType(Cmp->getOperand(0)->getType(), 32) &&
           isFixedIntVectorType(Cmp->getType(), 1);
  }
  if (const auto *Sel = dyn_cast<SelectInst>(&I)) {
    if (!Sel->getType()->isVectorTy())
      return true;
    return isFixedIntVectorType(Sel->getType(), 32) &&
           isFixedIntVectorType(Sel->getCondition()->getType(), 1);
  }
  return true;
}

bool isAllowedIRInstruction(const Instruction &I) {
  if (isa<InsertElementInst, ExtractElementInst>(&I))
    return true;
  if (isa<BranchInst, SwitchInst, ReturnInst, LoadInst, StoreInst,
          GetElementPtrInst, ZExtInst, SExtInst, TruncInst, ICmpInst, PHINode,
          SelectInst>(&I))
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
      if (unsigned Width = integerScalarWidth(BO->getType()))
        return isConstantIntOrVectorBelow(BO->getOperand(1), Width);
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
  if (!C || !C->getType()->isIntegerTy(32))
    return false;
  uint64_t Trip = C->getZExtValue();
  return Trip >= 1 && Trip <= 4;
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

bool isOrWithNonZeroI32Constant(const Value *V) {
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

bool triggersM024UDivSExtOr(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  return BO && BO->getOpcode() == Instruction::UDiv &&
         BO->getType()->isIntegerTy(32) &&
         isOrWithNonZeroI32Constant(BO->getOperand(1));
}

bool triggersM025URemSExtOr(const Instruction &I) {
  const auto *BO = dyn_cast<BinaryOperator>(&I);
  return BO && BO->getOpcode() == Instruction::URem &&
         BO->getType()->isIntegerTy(32) &&
         isOrWithNonZeroI32Constant(BO->getOperand(1));
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
  bool AllowM024 = envFlag("FUZZX_ALLOW_M024_UDIV_SEXT_OR", false);
  bool AllowM025 = envFlag("FUZZX_ALLOW_M025_UREM_SEXT_OR", false);
  bool AllowM026 = envFlag("FUZZX_ALLOW_M026_UMAX_XOR_AND_HIGHBIT", false);
  bool AllowM027 = envFlag("FUZZX_ALLOW_M027_XOR_AND_OR", false);
  bool AllowM028 = envFlag("FUZZX_ALLOW_M028_UMAX_AND_NOT", false);
  bool AllowM029 = envFlag("FUZZX_ALLOW_M029_FSHL_SELECT_PHI", false);
  bool AllowM030 = envFlag("FUZZX_ALLOW_M030_CTLZ_SHL_OR_BITOP3", false);
  bool AllowM031 = envFlag("FUZZX_ALLOW_M031_VECTOR_OR_EXTRACT_SUB", false);
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
              !isValidVectorInstruction(I) ||
              !isValidLoopControlInstruction(I) ||
              (!AllowM019 && triggersM019HighBitOrXor(I)) ||
              (!AllowM020 && triggersM020OrXorAnd(I)) ||
              (!AllowM021 && triggersM021OrXor(I)) ||
              (!AllowM022 && triggersM022AndXorConstant(I)) ||
              (!AllowM023 && triggersM023AndXorIdentity(I)) ||
              (!AllowM024 && triggersM024UDivSExtOr(I)) ||
              (!AllowM025 && triggersM025URemSExtOr(I)) ||
              (!AllowM026 && triggersM026UMaxXorAnd(I)) ||
              (!AllowM027 && triggersM027XorAndOr(I)) ||
              (!AllowM028 && triggersM028UMaxAndNot(I)) ||
              (!AllowM029 && triggersM029FshlSelectPhi(I)) ||
              (!AllowM030 && triggersM030CtlzShlOrBitop3(I)) ||
              (!AllowM031 && triggersM031VectorOrExtractSub(I)))
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

Value *emitVectorBuild(IRBuilder<NoFolder> &B, Type *VecTy,
                       ArrayRef<Value *> Elements) {
  Value *Result = Constant::getNullValue(VecTy);
  LLVMContext &Ctx = VecTy->getContext();
  for (unsigned I = 0, E = Elements.size(); I != E; ++I)
    Result = B.CreateInsertElement(Result, Elements[I], ci32(Ctx, I),
                                   "fuzz.vec.ins");
  return Result;
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
  switch (Gen() % 11) {
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

Value *emitRandomIRInstruction(IRBuilder<NoFolder> &B, Module &M,
                               Instruction *InsertPt, Value *Current,
                               std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I8 = Type::getInt8Ty(Ctx);
  Type *I16 = Type::getInt16Ty(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Value *A = Current;
  Value *Bv = chooseI32Value(InsertPt, Gen);
  switch (Gen() % 48) {
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
  default:
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
  Type *I32 = Type::getInt32Ty(Ctx);
  switch (Gen() % 24) {
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
  default:
    return emitRandomVectorInstruction(B, M, A, Bv, Gen);
  }
}

Value *emitRandomCFGLinearArm(IRBuilder<NoFolder> &B, Module &M,
                              Value *Current, Value *Other,
                              std::minstd_rand &Gen) {
  Value *Result = Current;
  unsigned Steps = 1 + (Gen() % 4);
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
  if (F.size() >= MaxIRCFGBlocks - 32)
    MaxDepth = 1;
  else if (F.size() >= 192)
    MaxDepth = std::min(MaxDepth, 2u);
  else if (F.size() >= 96)
    MaxDepth = std::min(MaxDepth, 3u);

  unsigned Depth = 1;
  while (Depth < MaxDepth && (Gen() % 3) != 0)
    ++Depth;
  return Depth;
}

Value *chooseCFGValue(LLVMContext &Ctx, Value *Other, std::minstd_rand &Gen) {
  return (Gen() % 2) == 0 ? Other : interestingI32(Ctx, Gen);
}

bool canGrowCFG(const Function &F) { return F.size() < MaxIRCFGBlocks; }

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

CFGFragment emitRandomNestedSwitch(IRBuilder<NoFolder> &B, Module &M,
                                   BasicBlock *InsertBefore, Value *Current,
                                   Value *Other, unsigned Depth,
                                   std::minstd_rand &Gen) {
  LLVMContext &Ctx = M.getContext();
  Type *I32 = Type::getInt32Ty(Ctx);
  Function *F = B.GetInsertBlock()->getParent();
  BasicBlock *Join =
      BasicBlock::Create(Ctx, "fuzz.nested.switch.join", F, InsertBefore);
  BasicBlock *Default =
      BasicBlock::Create(Ctx, "fuzz.nested.switch.default", F, Join);
  unsigned NumCases = 3 + (Gen() % 4);
  uint32_t Mask = NumCases <= 4 ? 3 : 7;

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

CFGFragment emitRandomCFGSubgraph(IRBuilder<NoFolder> &B, Module &M,
                                  BasicBlock *InsertBefore, Value *Current,
                                  Value *Other, unsigned Depth,
                                  std::minstd_rand &Gen) {
  Value *Linear = emitRandomCFGLinearArm(B, M, Current, Other, Gen);
  Function *F = B.GetInsertBlock()->getParent();
  if (Depth == 0 || !canGrowCFG(*F) || (Gen() % 5) == 0)
    return {Linear, B.GetInsertBlock()};

  Value *NestedOther = chooseCFGValue(M.getContext(), Other, Gen);
  if ((Gen() % 4) == 0)
    return emitRandomNestedSwitch(B, M, InsertBefore, Linear, NestedOther,
                                  Depth, Gen);
  return emitRandomNestedDiamond(B, M, InsertBefore, Linear, NestedOther,
                                 Depth, Gen);
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
      ThenB, M, Join, Current, Other, chooseCFGDepth(*F, 5, Gen), Gen);
  IRBuilder<NoFolder> ThenTailB(ThenFrag.Tail);
  ThenTailB.CreateBr(Join);

  IRBuilder<NoFolder> ElseB(Else);
  CFGFragment ElseFrag = emitRandomCFGSubgraph(
      ElseB, M, Join, Current, Other, chooseCFGDepth(*F, 5, Gen), Gen);
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
  unsigned NumCases = 3 + (Gen() % 4);
  uint32_t Mask = NumCases <= 4 ? 3 : 7;

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
        CaseB, M, Join, Current, CaseOther, chooseCFGDepth(*F, 4, Gen), Gen);
    IRBuilder<NoFolder> CaseTailB(CaseFrag.Tail);
    CaseTailB.CreateBr(Join);
    Phi->addIncoming(CaseFrag.Result, CaseFrag.Tail);
  }

  IRBuilder<NoFolder> DefaultB(Default);
  CFGFragment DefaultFrag = emitRandomCFGSubgraph(
      DefaultB, M, Join, Current, Other, chooseCFGDepth(*F, 4, Gen), Gen);
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
  unsigned TripCount = 1 + (Gen() % 4);

  BasicBlock *Preheader = Store->getParent();
  BasicBlock *Exit =
      Preheader->splitBasicBlock(Store->getIterator(), "fuzz.loop.exit");
  BasicBlock *Header =
      BasicBlock::Create(Ctx, "fuzz.loop.header", F, Exit);
  BasicBlock *Body = BasicBlock::Create(Ctx, "fuzz.loop.body", F, Exit);

  Instruction *OldTerm = Preheader->getTerminator();
  IRBuilder<NoFolder> PreB(OldTerm);
  PreB.CreateBr(Header);
  OldTerm->eraseFromParent();

  IRBuilder<NoFolder> HeaderB(Header);
  PHINode *Index = HeaderB.CreatePHI(I32, 2, "fuzz.loop.iv");
  PHINode *Acc = HeaderB.CreatePHI(I32, 2, "fuzz.loop.acc");
  Index->addIncoming(ci32(Ctx, 0), Preheader);
  Acc->addIncoming(Current, Preheader);
  Value *Done =
      HeaderB.CreateICmpULT(Index, ci32(Ctx, TripCount), "fuzz.loop.cond");
  HeaderB.CreateCondBr(Done, Body, Exit);

  IRBuilder<NoFolder> BodyB(Body);
  CFGFragment BodyFrag = emitRandomCFGSubgraph(
      BodyB, M, Exit, Acc, Other, chooseCFGDepth(*F, 3, Gen), Gen);
  IRBuilder<NoFolder> BodyTailB(BodyFrag.Tail);
  Value *NextIndex =
      BodyTailB.CreateAdd(Index, ci32(Ctx, 1), "fuzz.loop.next");
  BodyTailB.CreateBr(Header);
  Index->addIncoming(NextIndex, BodyFrag.Tail);
  Acc->addIncoming(BodyFrag.Result, BodyFrag.Tail);

  Store->setOperand(0, Acc);
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
    switch (Gen() % 15) {
    case 0:
    case 1:
    case 2:
      mutateIRAddInstruction(M, Gen);
      break;
    case 3:
    case 4:
    case 5:
      mutateIRAddDiamond(M, Gen);
      break;
    case 6:
    case 7:
    case 8:
      mutateIRAddSwitch(M, Gen);
      break;
    case 9:
    case 10:
      mutateIRAddCountedLoop(M, Gen);
      break;
    case 11:
    case 12:
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
  if (I.isTerminator() || I.mayReadOrWriteMemory() ||
      isa<GetElementPtrInst, PHINode>(&I))
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
