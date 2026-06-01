# c012 — Aggregate load/store lowered to forward memcpy loop miscompiles overlapping copies

- region: P2-atomic-aggrcopy-alloca
- file: NVPTXLowerAggrCopies.cpp 103-122
- kind: miscompile
- confidence(finder): 0.55

## Mechanism
NVPTXLowerAggrCopies collects single-use aggregate loads whose only user is a store of that loaded value (lines 70-81) when the loaded type's store size is >= MaxAggrCopySize (128 bytes). It then rewrites the pair `%v = load T, ptr %src; store T %v, ptr %dst` into a call to createMemCpyLoopKnownSize (lines 111-118) with CopyLen = DL.getTypeStoreSize(LI->getType()) and CanOverlap = true.

The problem: an IR `load T; store T` of an aggregate is FULLY DEFINED even when %src and %dst overlap -- the entire value is loaded before any byte is stored, so the store must observe the pre-store contents of the source region. createMemCpyLoopKnownSize, however, generates a plain FORWARD chunked copy loop (read src[i], write dst[i], i increasing) with memcpy semantics, which assumes the regions do not overlap. CanOverlap is only a metadata hint controlling whether alias_scope/noalias metadata is attached (LowerMemIntrinsics.cpp lines 450-462); it does NOT change the copy direction. So this path silently substitutes overlap-UB memcpy semantics for the well-defined load-then-store semantics.

Concretely, for `%dst = src+8`, store size 128, the forward loop does: i=0 writes dst[0]=src[8]:=src[0] (clobbering src[8]); later i=8 reads src[8] (now clobbered to original src[0]) and writes dst[8]=src[16]:=src[0] instead of the correct original src[8]. The destination bytes 8..127 should equal original src[0..119] but instead get a corrupted/replicated pattern. I confirmed with llc -mcpu=sm_70 -O2 that the emitted PTX is a single forward ld.b8/st.b8 loop with no src<dst direction check (unlike the memmove path, which correctly selects forward/backward).

The memmove lowering (expandMemMoveAsLoop) correctly handles overlap by emitting a runtime src<dst comparison and a backward loop; the aggregate load/store path bypasses that and is treated like a non-overlapping memcpy.

## Trigger
nvptx64 target, any sm_XX. An aggregate (array/struct) type whose store size is >= 128 bytes, loaded once and immediately stored to a pointer that overlaps the source with dst > src in the same address space. Reachable from valid (non-UB) IR because load;store of overlapping aggregates is well-defined in LLVM IR. The pass runs unconditionally in the NVPTX codegen pipeline (NVPTXTargetMachine.cpp:410).

## IR
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

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_70 -O2`
