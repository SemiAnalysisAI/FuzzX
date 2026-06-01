# 007 — Aggregate load/store lowered to forward memcpy loop miscompiles overlapping copies

- **Kind:** miscompile
- **Reachable via:** default llc (aggregate >=128B)
- **Component:** NVPTXLowerAggrCopies.cpp 103-122  (region `P2-atomic-aggrcopy-alloca`)
- **Candidate id:** c012

## Summary

overlapping aggregate `load;store` lowered to a forward memcpy loop (no overlap/direction handling)

## Mechanism / root cause

NVPTXLowerAggrCopies collects single-use aggregate loads whose only user is a store of that loaded value (lines 70-81) when the loaded type's store size is >= MaxAggrCopySize (128 bytes). It then rewrites the pair `%v = load T, ptr %src; store T %v, ptr %dst` into a call to createMemCpyLoopKnownSize (lines 111-118) with CopyLen = DL.getTypeStoreSize(LI->getType()) and CanOverlap = true.

The problem: an IR `load T; store T` of an aggregate is FULLY DEFINED even when %src and %dst overlap -- the entire value is loaded before any byte is stored, so the store must observe the pre-store contents of the source region. createMemCpyLoopKnownSize, however, generates a plain FORWARD chunked copy loop (read src[i], write dst[i], i increasing) with memcpy semantics, which assumes the regions do not overlap. CanOverlap is only a metadata hint controlling whether alias_scope/noalias metadata is attached (LowerMemIntrinsics.cpp lines 450-462); it does NOT change the copy direction. So this path silently substitutes overlap-UB memcpy semantics for the well-defined load-then-store semantics.

Concretely, for `%dst = src+8`, store size 128, the forward loop does: i=0 writes dst[0]=src[8]:=src[0] (clobbering src[8]); later i=8 reads src[8] (now clobbered to original src[0]) and writes dst[8]=src[16]:=src[0] instead of the correct original src[8]. The destination bytes 8..127 should equal original src[0..119] but instead get a corrupted/replicated pattern. I confirmed with llc -mcpu=sm_70 -O2 that the emitted PTX is a single forward ld.b8/st.b8 loop with no src<dst direction check (unlike the memmove path, which correctly selects forward/backward).

The memmove lowering (expandMemMoveAsLoop) correctly handles overlap by emitting a runtime src<dst comparison and a backward loop; the aggregate load/store path bypasses that and is treated like a non-overlapping memcpy.

## Trigger

nvptx64 target, any sm_XX. An aggregate (array/struct) type whose store size is >= 128 bytes, loaded once and immediately stored to a pointer that overlaps the source with dst > src in the same address space. Reachable from valid (non-UB) IR because load;store of overlapping aggregates is well-defined in LLVM IR. The pass runs unconditionally in the NVPTX codegen pipeline (NVPTXTargetMachine.cpp:410).

## Reproducer

See `repro.ll` / `cmd.sh`.

```
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "nvptx64-unknown-unknown"

define void @copy_overlap(ptr %p) {
entry:
  %dst = getelementptr inbounds i8, ptr %p, i64 8
  %v = load [128 x i8], ptr %p, align 1
  store [128 x i8] %v, ptr %dst, align 1
  ret void
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_70 -O2 -o - repro.ll
```

## Observed (wrong) output

```
$L__BB0_1:                              // %static-memcpy-expansion-main-body
	cvt.s64.s32 	%rd3, %r1;
	add.s64 	%rd4, %rd2, %rd3;        // src + i
	ld.b8 	%rs1, [%rd4];               // load src[i]
	add.s64 	%rd5, %rd1, %rd3;        // dst(=src+8) + i
	st.b8 	[%rd5], %rs1;               // store dst[i]
	add.s32 	%r1, %r1, 1;
	setp.lt.u32 	%p1, %r1, 128;       // only bound check, no src<dst direction check
	@%p1 bra 	$L__BB0_1;

Post-pass IR (-print-after=nvptx-lower-aggr-copies): a single FORWARD i8 copy loop
for loop-index 0..127: %1 = load i8, ptr (p + loop-index); store i8 %1, ptr (dst + loop-index)

For p with p[i]=i and dst=p+8: emitted loop yields p[16]=0 (because p[8] was clobbered to p[0]=0 at iteration 0 before being read at iteration 8). Same forward loop produced at -O0, confirming it is the backend pass.
```

## Expected

Because `load [128 x i8]; store [128 x i8]` reads the entire source value before storing any byte, an overlapping copy with dst>src must be performed as if backward (or buffered), giving dst[i] = ORIGINAL src[i]. For p[i]=i, dst=p+8: p[16] must become original p[8]=8 (and generally p[8+i]=i). A correct lowering would either emit a memmove-style runtime src<dst check selecting a backward loop for the dst>src case (as expandMemMoveAsLoop does), or otherwise preserve load-then-store semantics — not an unconditional forward memcpy loop. The bug is calling createMemCpyLoopKnownSize with CanOverlap=true for a load/store pair that may overlap; it should use overlap-safe (memmove-style) lowering.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.55, verify confidence 0.9).

> CONFIRMED MISCOMPILE. The mechanism in the candidate is real and empirically reproduced.

Source confirmation: NVPTXLowerAggrCopies.cpp lines 70-81 collect a single-use aggregate LoadInst whose only user is a StoreInst of the loaded value, when DL.getTypeStoreSize(LI->getType()) >= MaxAggrCopySize (128). Lines 103-122 rewrite that load/store pair via createMemCpyLoopKnownSize(..., CanOverlap=true). In LowerMemIntrinsics.cpp createMemCpyLoopKnownSize (lines 390-469) the main loop is an unconditional FORWARD chunked copy (read SrcAddr+index, write DstAddr+index, index increasing); CanOverlap only gates whether alias_scope/noalias metadata is attached (lines 450-462, 459-461) and does NOT change copy direction. Contrast: the memmove path (expandMemMoveAsLoop, comment at LowerMemIntrinsics.cpp 644-665) explicitly emits a runtime src<d check and a backward loop to handle overlap.

IR semantics vs emitted code: An IR `%v = load [128 x i8], ptr %src; store [128 x i8] %v, ptr %dst` is FULLY DEFINED even when src/dst overlap — load/store are plain byte memory ops with no overlap restriction (unlike memcpy). The whole 128-byte value is read (producing the SSA value) BEFORE any byte is stored, so the store must observe the pre-store source bytes. With dst=src+8, the correct result is dst[i]=original src[i], i.e. p[8+i]=original p[i].

The emitted PTX (and the post-pass IR) is a forward byte loop: for i=0..127: dst[i]=src[i], i.e. p[8+i]=p[i] reading p[i] AS-MODIFIED. This clobbers p[8..
