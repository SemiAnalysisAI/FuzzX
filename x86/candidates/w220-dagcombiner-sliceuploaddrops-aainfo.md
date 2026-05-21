# w220: DAGCombiner SliceUpLoad / LoadedSlice::loadSlice drops AAInfo (TBAA/alias.scope/noalias)

## Summary

`DAGCombiner::SliceUpLoad` splits a single integer load into multiple narrower
loads (when the load is consumed only via disjoint `trunc(lshr ...)` patterns).
The per-slice load is built by `LoadedSlice::loadSlice()`, which calls
`DAG.getLoad(...)` with the original load's `getMemOperand()->getFlags()` only.
It never forwards `Origin->getAAInfo()`, so the new narrow loads lose `!tbaa`,
`!alias.scope`, and `!noalias` metadata. `Origin->getRanges()` is also dropped.

This is a metadata-loss miscompile risk: subsequent MachineScheduler / DAGCombiner
alias queries can now reorder or fold loads that the IR explicitly told us
could not alias.

## Source

File: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp`
- `LoadedSlice::loadSlice()` at lines 21835-21865 — builds the per-slice
  `DAG.getLoad(...)` with only `Origin->getMemOperand()->getFlags()`:

```cpp
SDValue LastInst =
    DAG->getLoad(SliceType, SDLoc(Origin), Origin->getChain(), BaseAddr,
                 Origin->getPointerInfo().getWithOffset(Offset), getAlign(),
                 Origin->getMemOperand()->getFlags());
```

No `Origin->getAAInfo()` is passed (defaults to empty), no `Origin->getRanges()`
either. Compare to the nearby narrowing code in `DAGCombiner.cpp:16690-16696`
which DOES forward `LN0->getAAInfo()` and a (potentially truncated) `NewRanges`.

`SliceUpLoad` is invoked from `visitLOAD` at line 21603.

## Reproducer

```ll
; /tmp/w220/slice-tbaa.ll
target triple = "x86_64-unknown-linux-gnu"

define void @slice(ptr noalias %p, ptr %a, ptr %b) {
  %v = load i64, ptr %p, align 8, !tbaa !0, !alias.scope !4, !noalias !5
  %lo = trunc i64 %v to i32
  %shr = lshr i64 %v, 32
  %hi = trunc i64 %shr to i32
  store i32 %lo, ptr %a, align 4
  store i32 %hi, ptr %b, align 4
  ret void
}

!0 = !{!1, !1, i64 0}
!1 = !{!"long long", !2}
!2 = !{!"omnipotent char", !3}
!3 = !{!"Simple C/C++ TBAA"}
!4 = !{!6}
!5 = !{!7}
!6 = distinct !{!6, !8, !"scope1"}
!7 = distinct !{!7, !8, !"scope2"}
!8 = distinct !{!8, !"domain"}
```

## Repro command

```
llc -mtriple=x86_64-unknown-linux-gnu -O2 --combiner-stress-load-slicing \
    -stop-after=finalize-isel /tmp/w220/slice-tbaa.ll -o -
```

(The `--combiner-stress-load-slicing` flag forces the slicing decision; in
practice this transform fires whenever the cost heuristics approve. The
metadata loss is the same either way.)

## MIR difference

Without slicing:
```
%3:gr64 = MOV64rm ... :: (load (s64) from %ir.p, !tbaa !0, !alias.scope !4, !noalias !7)
```

With slicing:
```
%3:gr32 = MOV32rm %0, 1, $noreg, 0, $noreg :: (load (s32) from %ir.p, align 8)
%4:gr32 = MOV32rm %0, 1, $noreg, 4, $noreg :: (load (s32) from %ir.p + 4)
```

Note: `!tbaa`, `!alias.scope`, `!noalias` are gone from both new MMOs.

## Severity

Metadata loss. Downstream MI passes (MachineScheduler, MachineSink,
post-RA scheduling) use TBAA / alias.scope / noalias on MMOs to allow
reordering. After this combine, the slices look like generic memory accesses
and can be reordered against unrelated loads/stores that the front-end
explicitly proved non-aliasing.

## Suggested fix

In `LoadedSlice::loadSlice()`, replace the four-arg `getLoad` call with the
six-arg form that forwards `Origin->getAAInfo()` (and ideally a narrowed-range
metadata when the slice still represents a sub-range of the original load's
`!range`).
