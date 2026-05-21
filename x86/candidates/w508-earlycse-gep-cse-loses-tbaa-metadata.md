# EarlyCSE GEP CSE doesn't call combineMetadataForCSE (drops !tbaa / !noalias / !align on the removed GEP)

## Location
`llvm/lib/Transforms/Scalar/EarlyCSE.cpp` — GEP CSE block at lines 1688-1709. Line 1697 calls only `combineIRFlags` (which intersects inbounds/nuw/nusw), never `combineMetadataForCSE`.

```cpp
// EarlyCSE.cpp:1688
if (GEPValue::canHandle(&Inst)) {
  ...
  if (Value *V = AvailableGEPs.lookup(GEPVal)) {
    LLVM_DEBUG(dbgs() << "EarlyCSE CSE GEP: " << Inst << "  to: " << *V
                      << '\n');
    combineIRFlags(Inst, V);
    Inst.replaceAllUsesWith(V);
    ...
```

## Root cause
GetElementPtr instructions can carry metadata: `!llvm.access.group`, `!noalias`, `!alias.scope` (when paired with a memory operation), and most importantly `!noalias_addrspace` (LLVM 23 supports this on GEP-producing values). The load CSE path goes through `combineMetadataForCSE(I, &Inst, /*DoesKMove=*/false)` at line 1617 to merge these properly. The GEP CSE path skips that step.

Result: the kept GEP keeps only its own metadata; everything that the removed GEP carried is discarded. Where `combineMetadataForCSE` would have intersected (for !noalias / !alias.scope / !access_group with `DoesKMove=false` => "skip"; with `=true` => "intersect"), the GEP code does *nothing*.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

define ptr @f(ptr %p) {
  %g1 = getelementptr inbounds i32, ptr %p, i64 4
  %g2 = getelementptr inbounds i32, ptr %p, i64 4, !noalias !0
  %r = select i1 false, ptr %g1, ptr %g2
  ret ptr %r
}

!0 = !{!1}
!1 = distinct !{!1, !2, !"scope_A"}
!2 = distinct !{!"domain"}
```

## opt diff
Before:
```
%g1 = getelementptr inbounds i32, ptr %p, i64 4
%g2 = getelementptr inbounds i32, ptr %p, i64 4, !noalias !0
```

After `opt -passes='early-cse<memssa>' -S`:
```
%g1 = getelementptr inbounds i32, ptr %p, i64 4
; %g2 dropped; its !noalias is lost
```

## Why it is wrong
This is a metadata-information loss exactly analogous to w507 (call CSE) but for GEPs. The removed GEP's noalias/access-group/etc. facts are silently discarded. Strictly this is a missed optimization rather than a miscompile, but in cases where the kept GEP feeds an instruction that *was* derived using the removed GEP's !noalias context, downstream analysis loses real precision.

## Suggested fix
Call `combineMetadataForCSE(cast<Instruction>(V), &Inst, /*DoesKMove=*/false)` after `combineIRFlags(Inst, V)` at line 1697, mirroring the load CSE path.

## Status
REPRODUCIBLE at IR level. Metadata on the removed GEP is silently dropped after CSE.
