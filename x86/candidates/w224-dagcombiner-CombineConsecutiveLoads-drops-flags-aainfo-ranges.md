# w224: DAGCombiner CombineConsecutiveLoads drops MMO flags (nontemporal/invariant) and AAInfo on merged load

## Summary

`DAGCombiner::CombineConsecutiveLoads` fuses a `BUILD_PAIR(load(p),
load(p+sizeof(elt)))` into a single wider load. The new `getLoad` call uses
the six-argument form `getLoad(VT, dl, Chain, Ptr, PtrInfo, Align)` which
defaults `MachineMemOperand::Flags` to `MONone` and `AAMDNodes` to empty.

As a result the merged load loses:

- `MachineMemOperand::MONonTemporal`
- `MachineMemOperand::MOInvariant`
- `!tbaa`, `!alias.scope`, `!noalias`
- any `!range` metadata that was on the source loads

even when both source loads carried the same flags / metadata.

## Source

File: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp`
- `CombineConsecutiveLoads` at line 17581-17582:

```cpp
return DAG.getLoad(VT, SDLoc(N), LD1->getChain(), LD1->getBasePtr(),
                   LD1->getPointerInfo(), LD1->getAlign());
```

The eight-arg overload that takes `Flags` and `AAInfo` exists (see e.g. the
same file at line 14914 for `getLoad(...)` with both arguments).

## Reproducer

```ll
; /tmp/w220/consec-loads.ll
target triple = "x86_64-unknown-linux-gnu"

define i64 @t(ptr %p) {
  %p1 = getelementptr i8, ptr %p, i64 4
  %lo = load i32, ptr %p,  align 4, !tbaa !0, !nontemporal !10, !invariant.load !10
  %hi = load i32, ptr %p1, align 4, !tbaa !0, !nontemporal !10, !invariant.load !10
  %zlo = zext i32 %lo to i64
  %zhi = zext i32 %hi to i64
  %shi = shl i64 %zhi, 32
  %r = or i64 %zlo, %shi
  ret i64 %r
}
!0  = !{!1, !1, i64 0}
!1  = !{!"int", !2}
!2  = !{!"omnipotent char", !3}
!3  = !{!"Simple C/C++ TBAA"}
!10 = !{i32 1}
```

## Repro command

```
llc -mtriple=x86_64-unknown-linux-gnu -O2 \
    -stop-after=finalize-isel /tmp/w220/consec-loads.ll -o -
```

## MIR diff

Original IR:
```
%lo = load i32, ptr %p,  align 4, !tbaa !0, !invariant.load !4, !nontemporal !4
%hi = load i32, ptr %p1, align 4, !tbaa !0, !invariant.load !4, !nontemporal !4
```

After `CombineConsecutiveLoads`:
```
%1:gr64 = MOV64rm %0, 1, $noreg, 0, $noreg
  :: (load (s64) from %ir.p, align 4)
```

All of `non-temporal`, `invariant`, and `!tbaa` are gone from the MMO.

## Severity

- **`MOInvariant` loss** is the most consequential. If both source loads were
  invariant (immutable memory), downstream passes can hoist the merged load
  out of loops, CSE it freely, etc. Dropping `MOInvariant` blocks all of
  those optimizations.
- **`MONonTemporal` loss** drops a hardware caching hint that the source asked
  for. Observable cache behavior changes.
- **TBAA loss** weakens AA disambiguation in MachineScheduler / MachineSink.

## Suggested fix

Use the eight-arg `getLoad` overload:

```cpp
return DAG.getLoad(VT, SDLoc(N), LD1->getChain(), LD1->getBasePtr(),
                   LD1->getPointerInfo(), LD1->getAlign(),
                   LD1->getMemOperand()->getFlags() &
                       LD2->getMemOperand()->getFlags(),   // intersect
                   LD1->getAAInfo().concat(LD2->getAAInfo()));
```

(intersect MMO flags so `nontemporal`/`invariant` are kept only when both
sources have them; concat `AAInfo` to get the most permissive joint
metadata that is valid for either source location.)
