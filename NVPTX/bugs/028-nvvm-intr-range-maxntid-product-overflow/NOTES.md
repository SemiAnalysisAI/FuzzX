# 028 — NVVMIntrRange: maxntid dimension product truncated from uint64_t to unsigned, producing too-tight (or empty) tid/ntid ranges -> miscompile and assertion

- **Kind:** miscompile
- **Reachable via:** opt -passes=nvvm-intr-range / clang -O2 IR pipeline
- **Component:** NVVMIntrRange.cpp 79-93 (root cause at 79-80; consumed at 91-93 and via addRangeAttr 122-136)  (round-5 area `V04-sreg-fold-bound`)
- **Candidate id:** r5_01

## Summary

`nvvm.maxntid` whose dimension product exceeds UINT_MAX is truncated u64→unsigned before the 1024 clamp → tid/ntid folded to tiny wrong constants (or empty-range assert)

## Mechanism / root cause

getOverallMaxNTID(F) returns std::optional<uint64_t> = the PRODUCT of the maxntid dimensions (NVVMProperties.cpp:264 -> getVectorProduct accumulates into uint64_t). NVVMIntrRange.cpp:79-80 truncates this to `unsigned`:

  const unsigned MaxNTID = OverallMaxNTID.value_or(std::numeric_limits<unsigned>::max());

A maxntid product can legitimately exceed UINT_MAX (the per-dimension values are arbitrary unsigned ints, e.g. maxntid="641,6700417" with product 641*6700417 = 4294967297 = 2^32+1, a valid upper bound). After truncation MaxNTID becomes 1, so line 91-93 computes MaxBlockDim = {min(1024,1), min(1024,1), min(64,1)} = {1,1,1}. Then addRangeAttr gives tid.x range [0,1) (folded to constant 0) and ntid.x range [1,2) (folded to constant 1). The intended clamp `std::min(1024u, MaxNTID)` is defeated because the overflow happens BEFORE the min. Two manifestations of the same root cause: (a) product mod 2^32 in [1,1023] -> silently too-tight ranges -> MISCOMPILE; (b) product an exact multiple of 2^32 (e.g. maxntid="65536,65536,1", product=2^32 -> truncates to 0) -> ntid.x gets range [1,1) which is Lower==Upper, neither min nor max -> ASSERTION in ConstantRange::ConstantRange (ConstantRange.cpp:58) via addRangeAttr (line 56). Fix: keep MaxNTID as uint64_t (or clamp to 1024 before narrowing). Note this is distinct from the already-known literal maxntid-0-dim case: there the user writes 0 and tid.x gets benign [0,0); here a large legitimate value silently overflows.

## Trigger

ptx_kernel function with "nvvm.maxntid" whose dimension product exceeds UINT_MAX. maxntid="641,6700417" (product 2^32+1) -> tid.x folded to 0, ntid.x folded to 1; full -O2 collapses tid.x+ntid.x+tid.y+ntid.y to `ret i32 2` even though a valid launch (e.g. blockDim 256x256x1, product 65536 << 2^32+1) makes the true value up to 1022. maxntid="65536,65536,1" (product 2^32) -> assertion crash on ntid.x.

## Reproducer

```
define ptx_kernel i32 @overflow_to_one() "nvvm.maxntid"="641,6700417" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.tid.x()
  %2 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
  %3 = call i32 @llvm.nvvm.read.ptx.sreg.tid.y()
  %4 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.y()
  %5 = add i32 %1, %2
  %6 = add i32 %5, %3
  %7 = add i32 %6, %4
  ret i32 %7
}
declare i32 @llvm.nvvm.read.ptx.sreg.tid.x()
declare i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
declare i32 @llvm.nvvm.read.ptx.sreg.tid.y()
declare i32 @llvm.nvvm.read.ptx.sreg.ntid.y()
```

Command: `opt -S -mtriple=nvptx64-nvidia-cuda -passes=nvvm-intr-range -o - repro.ll`

## Observed (wrong) output

```
opt -O2 | llc emitted PTX:
.visible .entry overflow_to_one()
.maxntid 641, 6700417
{
// %bb.0:
	st.param.b32 	[func_retval0], 2;
	ret;
}

nvvm-intr-range pass attaches:
  %1 = call range(i32 0, 1) i32 @llvm.nvvm.read.ptx.sreg.tid.x()
  %2 = call range(i32 1, 2) i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
  %3 = call range(i32 0, 1) i32 @llvm.nvvm.read.ptx.sreg.tid.y()
  %4 = call range(i32 1, 2) i32 @llvm.nvvm.read.ptx.sreg.ntid.y()
opt -O2 (IR): define ... { ret i32 2 }

Assertion variant (maxntid="65536,65536,1"):
Assertion failed: ((Lower != Upper || (Lower.isMaxValue() || Lower.isMinValue())) && "Lower == Upper, but they aren't min or max value!"), function ConstantRange, file ConstantRange.cpp, line 58.
Stack: ConstantRange::ConstantRange -> addRangeAttr -> runNVVMIntrRange -> NVVMIntrRangePass::run   (exit code 134 / SIGABRT)
```

## Expected

The function must NOT be folded to a constant. Since maxntid bounds only the total thread count (here 2^32+1), tid.x and tid.y should each get range [0,1024) and ntid.x/ntid.y range [1,1025) (hardware-clamped, identical to the non-overflowing control maxntid="1000,1000"), leaving the add chain non-constant. For the concrete valid launch blockDim=1024x1x1 with tid.x=1023, the kernel returns 1023+1024+0+1 = 2048 (and up to 1022 for 256x256x1) -- not the emitted constant 2. The assertion variant must compile without crashing. Fix: keep MaxNTID as uint64_t, or clamp to 1024 before narrowing to unsigned (NVVMIntrRange.cpp:79-80).

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.97).

> Confirmed real. Root cause verified in source: getOverallMaxNTID(F) (NVVMProperties.cpp:257-264, via getVectorProduct line 204-210) accumulates the maxntid dimensions into a uint64_t product. NVVMIntrRange.cpp:79-80 then truncates this to `unsigned MaxNTID`. Because the truncation happens BEFORE the std::min(1024u, MaxNTID) clamp at lines 91-93, a product that exceeds UINT_MAX wraps and defeats the clamp.

MISCOMPILE (primary): maxntid="641,6700417" has product 641*6700417 = 4294967297 = 2^32+1, which truncates to unsigned 1. The nvvm-intr-range pass then attaches tid.x -> range(i32 0,1) (folds to const 0), ntid.x -> range(i32 1,2) (folds to const 1), tid.y -> range(i32 0,1), ntid.y -> range(i32 1,2). Full `opt -O2` folds the whole kernel to `ret i32 2`, and `opt -O2 | llc` emits `st.param.b32 [func_retval0], 2`.

This is wrong under the compiler's OWN semantic model. The source comment (NVVMProperties.cpp:258-263) quotes the PTX ISA: .maxntid bounds the TOTAL number of threads, not any per-dimension limit, and the code accordingly uses only the product. With total <= 2^32+1, a valid launch of blockDim 1024x1x1 (total 1024 << 2^32+1) makes tid.x=1023, ntid.x=1024, tid.y=0, ntid.y=1
