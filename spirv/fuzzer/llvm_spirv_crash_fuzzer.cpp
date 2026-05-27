#include "llvm/Bitcode/BitcodeReader.h"
#include "llvm/IR/Function.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/LegacyPassManager.h"
#include "llvm/IR/LLVMContext.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/Verifier.h"
#include "llvm/MC/TargetRegistry.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Support/CodeGen.h"
#include "llvm/Support/CrashRecoveryContext.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/ErrorHandling.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/TargetSelect.h"
#include "llvm/Support/raw_ostream.h"
#include "llvm/Target/TargetMachine.h"
#include "llvm/Target/TargetOptions.h"
#include "llvm/TargetParser/Triple.h"

#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <unistd.h>

using namespace llvm;

namespace {

constexpr StringRef DefaultTriple = "spirv64-unknown-unknown";

bool envFlag(const char *Name, bool Default) {
  const char *Value = std::getenv(Name);
  if (!Value || !*Value)
    return Default;
  return std::strcmp(Value, "0") != 0 && std::strcmp(Value, "false") != 0 &&
         std::strcmp(Value, "False") != 0 && std::strcmp(Value, "no") != 0 &&
         std::strcmp(Value, "off") != 0;
}

const Target *getSPIRVTarget() {
  static const Target *T = [] {
    LLVMInitializeSPIRVTargetInfo();
    LLVMInitializeSPIRVTarget();
    LLVMInitializeSPIRVTargetMC();
    LLVMInitializeSPIRVAsmPrinter();

    std::string Error;
    Triple TT(DefaultTriple);
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

  Triple TT(DefaultTriple);
  TargetOptions Options;
  TM.reset(getSPIRVTarget()->createTargetMachine(
      TT, CPU, "", Options, std::nullopt, std::nullopt,
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
  return (Dir / ("fuzzx-spirv-crash-" + std::to_string(getpid()) + "-" +
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

StringRef getCPU() {
  const char *Env = std::getenv("FUZZX_SPIRV_CPU");
  if (Env && *Env)
    return Env;
  return "";
}

std::unique_ptr<Module> createIRSkeletonModule(LLVMContext &Ctx,
                                               StringRef /*CPU*/) {
  auto M = std::make_unique<Module>("fuzzx_spirv_crash", Ctx);
  M->setTargetTriple(Triple(DefaultTriple));

  Type *VoidTy = Type::getVoidTy(Ctx);
  Type *I32 = Type::getInt32Ty(Ctx);
  Type *GlobalPtr = PointerType::get(Ctx, 1);
  FunctionType *FT =
      FunctionType::get(VoidTy, {GlobalPtr, GlobalPtr, I32}, false);
  Function *F = Function::Create(FT, GlobalValue::ExternalLinkage,
                                 "fuzz_kernel", *M);
  F->setCallingConv(CallingConv::SPIR_KERNEL);

  BasicBlock *Entry = BasicBlock::Create(Ctx, "entry", F);
  BasicBlock *Body = BasicBlock::Create(Ctx, "body", F);
  BasicBlock *Exit = BasicBlock::Create(Ctx, "exit", F);

  IRBuilder<> B(Entry);
  Argument *In = F->getArg(0);
  Argument *Out = F->getArg(1);
  Argument *N = F->getArg(2);
  Value *Ok = B.CreateICmpSGT(N, ConstantInt::get(I32, 0));
  B.CreateCondBr(Ok, Body, Exit);

  B.SetInsertPoint(Body);
  Value *V = B.CreateAlignedLoad(I32, In, Align(4));
  Value *Salt = B.CreateMul(N, ConstantInt::getSigned(I32, -1640531527));
  Value *Mix = B.CreateXor(V, Salt);
  B.CreateAlignedStore(Mix, Out, Align(4));
  B.CreateBr(Exit);

  B.SetInsertPoint(Exit);
  B.CreateRetVoid();
  return M;
}

bool validateIRCorpusModule(const Module &M) {
  if (M.empty())
    return false;
  for (const Function &F : M) {
    if (F.isDeclaration())
      continue;
    if (F.getCallingConv() != CallingConv::SPIR_KERNEL &&
        F.getCallingConv() != CallingConv::SPIR_FUNC &&
        F.getCallingConv() != CallingConv::C)
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
  MemoryBufferRef MemBuf(Buffer, "fuzzx-spirv-ir-bitcode");
  std::unique_ptr<Module> Parsed;
  // BitcodeReader is not hardened against arbitrary mutated bytes; trap its
  // assertions so we report only SPIR-V backend findings.
  CrashRecoveryContext CRC;
  CRC.RunSafely([&]() {
    Expected<std::unique_ptr<Module>> P = parseBitcodeFile(MemBuf, Ctx);
    if (!P) {
      consumeError(P.takeError());
      return;
    }
    Parsed = std::move(*P);
  });
  if (!Parsed)
    return createIRSkeletonModule(Ctx, CPU);
  // Force the triple so corpus mutation of target metadata cannot send us to
  // a different backend.
  Parsed->setTargetTriple(Triple(DefaultTriple));
  if (!validateIRCorpusModule(*Parsed))
    return createIRSkeletonModule(Ctx, CPU);
  if (Valid)
    *Valid = true;
  return Parsed;
}

void saveFailureFinding(const uint8_t *Data, size_t Size, StringRef IRText,
                        StringRef Kind, StringRef Stage,
                        std::optional<int> CrashRetCode = std::nullopt) {
  const char *FindingsDir = std::getenv("FUZZX_FINDINGS_DIR");
  if (!FindingsDir || !*FindingsDir)
    return;
  std::error_code EC;
  std::filesystem::create_directories(FindingsDir, EC);
  auto Now = std::chrono::steady_clock::now().time_since_epoch().count();
  std::string Base = std::string(FindingsDir) + "/" + Kind.str() + "-" +
                     Stage.str() + "-" + std::to_string(getpid()) + "-" +
                     std::to_string(Now);
  if (CrashRetCode)
    Base += "-rc" + std::to_string(*CrashRetCode);
  std::ofstream BC(Base + ".bc", std::ios::binary);
  if (BC)
    BC.write(reinterpret_cast<const char *>(Data),
             static_cast<std::streamsize>(Size));
  if (!IRText.empty()) {
    std::ofstream LL(Base + ".ll");
    if (LL)
      LL.write(IRText.data(), static_cast<std::streamsize>(IRText.size()));
  }
}

CompileResult compileIRModuleToObject(Module &M, StringRef CPU,
                                      OptimizationLevel Level,
                                      std::string *IRText = nullptr) {
  CompileResult R;
  TargetMachine *TM = getTargetMachine(CPU, Level);
  if (!TM) {
    R.FailureStage = "target-machine";
    return R;
  }
  CrashRecoveryContext CRC;
  CRC.DumpStackAndCleanupOnFailure = true;
  bool Ran = CRC.RunSafely([&]() {
    if (!runOptimizationPipeline(M, *TM, Level)) {
      R.FailureStage = "opt";
      return;
    }
    if (IRText)
      *IRText = moduleToString(M);
    auto Obj = emitObject(M, *TM);
    if (!Obj) {
      R.FailureStage = "codegen";
      return;
    }
    R.Object = std::move(*Obj);
    R.Success = true;
  });
  if (!Ran) {
    R.Crashed = true;
    R.CrashRetCode = -1;
    if (R.FailureStage.empty())
      R.FailureStage = "codegen";
  }
  return R;
}

} // namespace

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

  std::string O0IR;
  auto O0Obj = compileIRModuleToObject(*M, CPU, OptimizationLevel::O0, &O0IR);
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
      if (O0Obj.Crashed)
        std::abort();
    }
    return 0;
  }

  // Re-parse a fresh copy for O2 since the O0 pipeline mutated the module.
  ValidInput = false;
  std::unique_ptr<Module> M2 =
      parseIRCorpusModule(Data, Size, Ctx, CPU, &ValidInput);
  if (!ValidInput)
    return 0;

  std::string O2IR;
  auto O2Obj = compileIRModuleToObject(*M2, CPU, OptimizationLevel::O2, &O2IR);
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
      if (O2Obj.Crashed)
        std::abort();
    }
    return 0;
  }

  // SPIR-V has no host runtime, so we stop after codegen.  See ../README.md
  // for why a faithful differential port (the AMDGPU/PTX pattern) requires
  // setting up a Vulkan or OpenCL ICD.
  (void)envFlag;
  (void)writeBytes;
  (void)tempPath;
  return 0;
}

extern "C" int LLVMFuzzerInitialize(int *, char ***) {
  CrashRecoveryContext::Enable();
  install_fatal_error_handler(
      [](void *, const char *Reason, bool) {
        errs() << "FuzzX SPIR-V fatal: " << Reason << "\n";
        std::abort();
      },
      nullptr);
  install_bad_alloc_error_handler(
      [](void *, const char *Reason, bool) {
        errs() << "FuzzX SPIR-V bad alloc: " << Reason << "\n";
        std::abort();
      },
      nullptr);
  StringRef CPU = getCPU();
  (void)getTargetMachine(CPU, OptimizationLevel::O0);
  (void)getTargetMachine(CPU, OptimizationLevel::O2);
  return 0;
}
