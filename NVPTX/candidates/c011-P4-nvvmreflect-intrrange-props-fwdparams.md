# c011 — NVVMIntrRange: addRangeAttr range upper bound overflows 32-bit APInt for cluster.nctarank when maxclusterrank == UINT32_MAX (assertion in +asserts, silent wrapped/empty range in release)

- region: P4-nvvmreflect-intrrange-props-fwdparams
- file: NVVMIntrRange.cpp 46-58, 109-117, 156-158
- kind: assertion
- confidence(finder): 0.6

## Mechanism
addRangeAttr builds the upper bound with `APInt(BitWidth, High)` where BitWidth is 32 (the intrinsic return type is i32) and High is a uint64_t. The nctarank case calls `addRangeAttr(MinClusterSize, MaxClusterSize + 1, II)`. In the no-cluster_dim branch, `MaxClusterSize = MaxNctaPerCluster = MaxClusterRank.value_or(UINT_MAX)`. If the function has `"nvvm.maxclusterrank"="4294967295"` (0xffffffff), then `MaxClusterSize + 1 == 0x100000000`, and `APInt(32, 0x100000000)` triggers `assert(isUIntN(BitWidth, val) && "Value is not an N-bit unsigned value")` (APInt.h:128). I reproduced this crash with opt -passes=nvvm-intr-range. In a release (NDEBUG) build the assert is gone and the value silently truncates to 0, producing the range `[1, 0)` for nctarank; the same +1 overflow pattern in the ntid.*/cluster_nctaid.* cases is what makes this class of off-by-one-at-type-max a latent range-corruption bug. Code excerpt: `static bool addRangeAttr(uint64_t Low, uint64_t High, IntrinsicInst *II){ const uint64_t BitWidth = II->getType()->getIntegerBitWidth(); ConstantRange Range(APInt(BitWidth, Low), APInt(BitWidth, High)); ... }` and `case nvvm_read_ptx_sreg_cluster_nctarank: return HasClusterInfo && addRangeAttr(MinClusterSize, MaxClusterSize + 1, II);`. The fix would be to cap High at the type max (or build the APInt at a wider width / clamp before +1).

## Trigger
Kernel function with attribute "nvvm.maxclusterrank"="4294967295" (UINT32_MAX) that calls llvm.nvvm.read.ptx.sreg.cluster.nctarank(). Run the nvvm-intr-range pass (opt -passes=nvvm-intr-range, or clang's NVPTX IR pipeline). Asserts build: crash; release build: silently wrong/wrapped range attribute on the intrinsic.

## IR
```
declare i32 @llvm.nvvm.read.ptx.sreg.cluster.nctarank()

define ptx_kernel i32 @k() "nvvm.maxclusterrank"="4294967295" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.cluster.nctarank()
  ret i32 %1
}
```

## llc cmd
`opt -mtriple=nvptx64 -mcpu=sm_90 -passes=nvvm-intr-range -S  (reproduces the assertion; this pass is not in the bare `llc` codegen pipeline, it is run by clang / explicitly via opt)`
