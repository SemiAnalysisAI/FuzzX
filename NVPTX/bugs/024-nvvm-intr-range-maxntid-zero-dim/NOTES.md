# 024 — NVVMIntrRange: maxntid (or reqntid) with a 0 dimension creates ConstantRange(L,L) -> assertion crash on valid IR

- **Kind:** crash (assert/UB)
- **Reachable via:** opt -passes=nvvm-intr-range (clang IR pipeline)
- **Component:** NVVMIntrRange.cpp 79-93, 131-136 (addRangeAttr at 46-58)  (round-4 area `U11-nvvmreflect-reqntid`)
- **Candidate id:** r4_04

## Summary

`nvvm.maxntid`/`reqntid` with a 0 dimension builds an empty ConstantRange → assert in NVVMIntrRange

## Mechanism / root cause

When a kernel has nvvm.maxntid with a zero component (e.g. "0" or "64,0,1"), getOverallMaxNTID returns the PRODUCT = 0, so MaxNTID=0 and MaxBlockDim.X = std::min(1024u, 0) = 0. The ntid.x case at line 132 calls addRangeAttr(MinBlockDim.X /*=1*/, MaxBlockDim.X + 1 /*=1*/, II). addRangeAttr (line 51) builds ConstantRange(APInt(32,1), APInt(32,1)). ConstantRange's ctor asserts ((Lower != Upper || Lower.isMaxValue() || Lower.isMinValue())); Lower==Upper==1 is neither min nor max => assertion fires. (The tid.x case addRangeAttr(0,0) is allowed because 0 is the min value, so the crash is specifically on the ntid path.) Root cause: the pass treats maxntid/reqntid dimension values as exact upper/lower bounds without validating against 0; a single 0 makes the product-derived bound 0 and yields a degenerate [L,L) range with L=1. The same shape arises for reqntid (e.g. reqntid="0" yields ntid.x range [0,1) intersected with the intrinsic's declared [1,1025) => empty [0,0)). In a release (no-assert) build, ConstantRange(1,1) is silently constructed as the full/empty set, degrading the assert into a wrong range. This pass runs in the clang -O{1,2,3} optimization pipeline (registerPipelineStartEPCallback in NVPTXTargetMachine.cpp:239-253), so the crash reproduces in normal clang CUDA compilation, not just standalone opt.

## Trigger

A ptx_kernel function with attribute nvvm.maxntid="0" (e.g. user writing __launch_bounds__(0)) that calls @llvm.nvvm.read.ptx.sreg.ntid.x(), compiled at -O1/-O2/-O3 (or opt -passes=nvvm-intr-range). Also triggered by any maxntid with a zero component such as "64,0,1".

## Reproducer

```
define ptx_kernel i32 @t() "nvvm.maxntid"="0" {
  %ntid.x = call i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
  ret i32 %ntid.x
}
declare i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
```

Command: `opt -S -mtriple=nvptx64-nvidia-cuda -mcpu=sm_90 -passes=nvvm-intr-range -o - (also reproduces with -passes='default<O2>'; note: this is opt, not llc — the pass runs in the optimization pipeline, so plain llc codegen does NOT crash) repro.ll`

## Observed (wrong) output

```
Assertion failed: ((Lower != Upper || (Lower.isMaxValue() || Lower.isMinValue())) && "Lower == Upper, but they aren't min or max value!"), function ConstantRange, file ConstantRange.cpp, line 58.
Stack dump:
1.	Running pass "function(nvvm-intr-range)" on module ".../r4_04.ll"
2.	Running pass "nvvm-intr-range" on function "t"
 #7 llvm::ConstantRange::ConstantRange(llvm::APInt, llvm::APInt)
 #8 addRangeAttr(unsigned long long, unsigned long long, llvm::IntrinsicInst*)
 #9 runNVVMIntrRange(llvm::Function&)
#10 llvm::NVVMIntrRangePass::run(llvm::Function&, llvm::AnalysisManager<llvm::Function>&)
```

## Expected

The pass should not construct a degenerate [1,1) ConstantRange. A maxntid (or maxntid component) of 0 makes the product-derived MaxNTID 0, but the code unconditionally computes the ntid.* upper bound as MaxBlockDim+1 with MinBlockDim=1, yielding the invalid range [1,1). The pass should guard against the zero-derived bound — e.g. treat a 0 overall-maxntid as "no constraint" (skip adding a range, as for the control maxntid=64 which correctly yields range(i32 1, 65)), or clamp so Lower < Upper — rather than asserting/UB in ConstantRange. No crash should occur on this valid, verifier-accepted IR.

## Verification

Independent verify + adversarial refute confirmed 

> Confirmed real assertion crash on valid IR, reproduced with the assertions-enabled build at /Users/justinlebar/code/llvm2/build/bin/opt.

Source mechanism verified: getOverallMaxNTID (NVVMProperties.cpp:264) returns the PRODUCT of the maxntid vector (getVectorProduct accumulates with multiplies). A zero component yields product 0. In NVVMIntrRange.cpp:79-92, MaxNTID=0 -> MaxBlockDim.X = std::min(1024u,0) = 0. The ntid.x case (line 132) calls addRangeAttr(MinBlockDim.X=1, MaxBlockDim.X+1=1, II), which builds ConstantRange(APInt(32,1), APInt(32,1)). The ConstantRange ctor (ConstantRange.cpp:51-59) asserts (Lower != Upper || Lower.isMaxValue() || Lower.isMinValue()); Lower==Upper==1 is neither min(0) nor max(0xFFFFFFFF), so the assert fires. Real stack trace goes addRangeAttr -> ConstantRange::ConstantRange(APInt,APInt).

Validity confirmed: opt -passes=verify accepts "nvvm.maxntid"="0" with exit 0 and no diagnostic; there is no validation anywhere in the NVPTX backend requiring maxntid components to be nonzero. So this is valid IR, not UB.

Pipeline: The pass runs via registerPipelineStartEPCallback (NVPTXTargetMachine.cpp:239-253), so it fires in the opt optimization pipeline (opt -
