# 012 — NVVMIntrRange: addRangeAttr range upper bound overflows 32-bit APInt for cluster.nctarank when maxclusterrank == UINT32_MAX (assertion in +asserts, silent wrapped/empty range in release)

- **Kind:** crash (assert/UB)
- **Reachable via:** opt -passes=nvvm-intr-range (clang IR pipeline)
- **Component:** NVVMIntrRange.cpp 46-58, 109-117, 156-158  (region `P4-nvvmreflect-intrrange-props-fwdparams`)
- **Candidate id:** c011

## Summary

`nvvm.maxclusterrank=UINT32_MAX` overflows a 32-bit APInt in NVVMIntrRange

## Mechanism / root cause

addRangeAttr builds the upper bound with `APInt(BitWidth, High)` where BitWidth is 32 (the intrinsic return type is i32) and High is a uint64_t. The nctarank case calls `addRangeAttr(MinClusterSize, MaxClusterSize + 1, II)`. In the no-cluster_dim branch, `MaxClusterSize = MaxNctaPerCluster = MaxClusterRank.value_or(UINT_MAX)`. If the function has `"nvvm.maxclusterrank"="4294967295"` (0xffffffff), then `MaxClusterSize + 1 == 0x100000000`, and `APInt(32, 0x100000000)` triggers `assert(isUIntN(BitWidth, val) && "Value is not an N-bit unsigned value")` (APInt.h:128). I reproduced this crash with opt -passes=nvvm-intr-range. In a release (NDEBUG) build the assert is gone and the value silently truncates to 0, producing the range `[1, 0)` for nctarank; the same +1 overflow pattern in the ntid.*/cluster_nctaid.* cases is what makes this class of off-by-one-at-type-max a latent range-corruption bug. Code excerpt: `static bool addRangeAttr(uint64_t Low, uint64_t High, IntrinsicInst *II){ const uint64_t BitWidth = II->getType()->getIntegerBitWidth(); ConstantRange Range(APInt(BitWidth, Low), APInt(BitWidth, High)); ... }` and `case nvvm_read_ptx_sreg_cluster_nctarank: return HasClusterInfo && addRangeAttr(MinClusterSize, MaxClusterSize + 1, II);`. The fix would be to cap High at the type max (or build the APInt at a wider width / clamp before +1).

## Trigger

Kernel function with attribute "nvvm.maxclusterrank"="4294967295" (UINT32_MAX) that calls llvm.nvvm.read.ptx.sreg.cluster.nctarank(). Run the nvvm-intr-range pass (opt -passes=nvvm-intr-range, or clang's NVPTX IR pipeline). Asserts build: crash; release build: silently wrong/wrapped range attribute on the intrinsic.

## Reproducer

See `repro.ll` / `cmd.sh`.

```
declare i32 @llvm.nvvm.read.ptx.sreg.cluster.nctarank()

define ptx_kernel i32 @k() "nvvm.maxclusterrank"="4294967295" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.cluster.nctarank()
  ret i32 %1
}
```

Command:

```
opt -mtriple=nvptx64 -mcpu=sm_90 -passes=nvvm-intr-range -S -o - repro.ll
```

## Observed (wrong) output

```
Assertion failed: (llvm::isUIntN(BitWidth, val) && "Value is not an N-bit unsigned value"), function APInt, file APInt.h, line 128.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: .../opt -mtriple=nvptx64 -mcpu=sm_90 -passes=nvvm-intr-range -S c011.ll -o -
1.	Running pass "function(nvvm-intr-range)" on module "c011.ll"
2.	Running pass "nvvm-intr-range" on function "k"
 #7 ... addRangeAttr(unsigned long long, unsigned long long, llvm::IntrinsicInst*)
 #8 ... runNVVMIntrRange(llvm::Function&)
 #9 ... llvm::NVVMIntrRangePass::run(llvm::Function&, llvm::AnalysisManager<llvm::Function>&)

(Boundary confirmation: maxclusterrank=4294967294 does NOT crash and emits "call range(i32 1, -1) i32 @llvm.nvvm.read.ptx.sreg.cluster.nctarank()", i.e. range [1, 0xffffffff). The off-by-one to 4294967295 is what overflows i32.)
```

## Expected

No crash. The pass should clamp the range upper bound to the i32 type maximum (or build the APInt at a wider width before applying +1). For nvvm.maxclusterrank=4294967295 (UINT32_MAX), the correct attribute is a saturated range such as range(i32 1, 0) meaning [1, 2^32) over the full i32 domain — i.e. High should be clamped so APInt(32, High) is well-formed rather than computing MaxClusterSize+1 = 0x100000000 which is not representable in 32 bits.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.6, verify confidence 0.95).

> CONFIRMED assertion bug, reproduced with the built tools.

Mechanism (verified against NVVMIntrRange.cpp): addRangeAttr (lines 46-58) builds the range's upper bound as APInt(BitWidth=32, High) where High is uint64_t. For nvvm_read_ptx_sreg_cluster_nctarank (lines 156-158) it is called as addRangeAttr(MinClusterSize, MaxClusterSize + 1, II). In the no-cluster_dim branch (lines 109-117) MaxClusterSize = MaxNctaPerCluster = MaxClusterRank.value_or(UINT_MAX) and is NOT capped (unlike ntid which is capped at 1024 on line 91, and cluster_nctaid dims capped at 0x7fffffff/0xffff on lines 112-114). With "nvvm.maxclusterrank"="4294967295" (UINT32_MAX), MaxClusterSize+1 = 0x100000000, so APInt(32, 0x100000000) fails assert(isUIntN(BitWidth, val) && "Value is not an N-bit unsigned value") at APInt.h:128.

Validity / adversarial checks:
- The IR passes the verifier cleanly (opt -passes=verify succeeds); the string attribute "nvvm.maxclusterrank"="4294967295" is accepted with no range constraint. No UB, poison, or freeze involved. The crash is deterministic on well-formed IR.
- Boundary pinned exactly: maxclusterrank=4294967294 does NOT crash (emits range(i32 1, -1) = [1, 0xffffffff)); maxclusterrank=4294967295 crashes. This isolates the failure to the MaxClusterSize+1 overflow exactly at type-max, as claimed.
- Reachability: the pass is registered via registerPipelineStartEPCallback in NVPTXTargetMachine.cpp:249, so it runs in the standard clang/opt NVPTX optimization pipeline (not the ba
