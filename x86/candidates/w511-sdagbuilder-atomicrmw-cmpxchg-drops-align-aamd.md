# w511 - `SelectionDAGBuilder::visitAtomicRMW` / `visitAtomicCmpXchg` drop the user-specified `align` and ALL AAMD metadata on the MMO

## Location

`llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp` -
`SelectionDAGBuilder::visitAtomicCmpXchg`
(lines 5196-5226, MMO built at 5211-5214)
and
`SelectionDAGBuilder::visitAtomicRMW`
(lines 5228-5296, MMO built at 5283-5285).

Both builders compute the MachineMemOperand with these arguments
(simplified, with the offending values flagged):

```c++
MachineMemOperand *MMO = MF.getMachineMemOperand(
    MachinePointerInfo(I.getPointerOperand()),
    Flags,
    MemVT.getStoreSize(),
    DAG.getEVTAlign(MemVT),  // <-- NOT I.getAlign()
    AAMDNodes(),             // <-- empty!  TBAA, alias.scope, noalias dropped
    nullptr,                 // ranges dropped
    SSID,
    Ordering);
```

Compare against the sibling builders for the non-RMW atomics:

| visitor                  | alignment argument          | AAMD argument         |
| ------------------------ | --------------------------- | --------------------- |
| `visitLoad` (4778)       | `I.getAlign()`              | `AAInfo`              |
| `visitStore` (4934)      | `I.getAlign()`              | `AAInfo`              |
| `visitAtomicLoad` (5332) | `I.getAlign()`              | `AAMDNodes()` (also a bug) |
| `visitAtomicStore` (5369)| `I.getAlign()`              | `AAMDNodes()` (also a bug) |
| `visitAtomicRMW` (5285)  | `DAG.getEVTAlign(MemVT)`    | `AAMDNodes()`         |
| `visitAtomicCmpXchg` (5213)| `DAG.getEVTAlign(MemVT)`  | `AAMDNodes()`         |

The two RMW builders are the ONLY memory-touching builders that
both (a) ignore `I.getAlign()` and (b) provide an empty `AAMDNodes`.

`AtomicRMWInst::getAlign` (and `AtomicCmpXchgInst::getAlign`) exist
specifically to let the IR producer over-align the location relative
to the type's natural alignment - useful both for the optimizer (`%p`
proved to be 32-byte aligned) and for downstream codegen that has
alignment-dependent strategies. Currently that information is
silently truncated to the EVT's natural alignment as soon as the
SDAG MMO is created.

## Repro

`atomic_align.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"

define i32 @arm_align(ptr %p) {
  %v = atomicrmw add ptr %p, i32 5 seq_cst, align 32
  ret i32 %v
}

define {i32, i1} @cmpxchg_align(ptr %p, i32 %cmp, i32 %new) {
  %v = cmpxchg ptr %p, i32 %cmp, i32 %new seq_cst seq_cst, align 32
  ret {i32, i1} %v
}

define i32 @load_align(ptr %p) {
  %v = load atomic i32, ptr %p seq_cst, align 32
  ret i32 %v
}
```

`atomic_aa.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @arm_aa(ptr %p) {
  %v = atomicrmw add ptr %p, i32 5 seq_cst, !tbaa !0, !alias.scope !4, !noalias !7
  ret i32 %v
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C/C++ TBAA"}
!4 = !{!5}
!5 = distinct !{!5, !6}
!6 = distinct !{!6, !"foo"}
!7 = !{!8}
!8 = distinct !{!8, !6}
```

## Invocation

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel atomic_align.ll
llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel atomic_aa.ll
```

## Observed (alignment lost)

```
%v = atomicrmw add ptr %p, i32 5 seq_cst, align 32
  -> LXADD32 ... :: (load store seq_cst (s32) on %ir.p)           # no "align 32"

%v = cmpxchg ptr %p, i32 %cmp, i32 %new seq_cst seq_cst, align 32
  -> LCMPXCHG32 ... :: (load store seq_cst seq_cst (s32) on %ir.p)# no "align 32"

# Control: load atomic correctly preserves the over-alignment.
%v = load atomic i32, ptr %p seq_cst, align 32
  -> MOV32rm ... :: (load seq_cst (s32) from %ir.p, align 32)
```

## Observed (AAMD lost)

```
%v = atomicrmw add ptr %p, i32 5 seq_cst, !tbaa !0, !alias.scope !4, !noalias !7
  -> LXADD32 ... :: (load store seq_cst (s32) on %ir.p)           # no !tbaa, no !alias.scope, no !noalias
```

For comparison, a plain `store` keeps all three:

```
store i32 %v, ptr %p, !tbaa !0, !alias.scope !4, !noalias !7
  -> MOV32mr ... :: (store (s32) into %ir.p, !tbaa !0, !alias.scope !4, !noalias !7)
```

## Why it matters

1. Alignment loss makes downstream MIR layer alignment-aware decisions
   conservative. On x86 the cost of an unaligned atomic vs aligned
   atomic is observable on cache-line-split cases; an
   `align 32` `cmpxchg16b` candidate could legally be emitted but the
   MIR layer no longer sees the alignment, so it conservatively assumes
   only natural alignment.
2. AAMD loss breaks all post-isel AA queries on atomic memory:
   `MachineMemOperand::getAAInfo()` returns the empty `AAMDNodes()`,
   so passes like `MachineLICM`, `MachineSink`, scheduling and the
   `MachineMemOperand::aliasingHints` / `mayAlias` family see two atomic
   ops to genuinely different objects as "may alias" even when the IR
   has proved otherwise with TBAA / alias.scope.
3. For atomicrmw with a `!nontemporal` hint (allowed by the verifier
   today), the bit is also discarded because
   `TLI.getAtomicMemOperandFlags` does not look at the metadata
   (`TargetLoweringBase.cpp:2829-2846`, see FIXME on line 2843).

## Where the data is lost

- `SelectionDAGBuilder.cpp:5213` (cmpxchg): `DAG.getEVTAlign(MemVT)` + `AAMDNodes()`.
- `SelectionDAGBuilder.cpp:5285` (atomicrmw): same.
- `TargetLoweringBase.cpp:2829-2846` `getAtomicMemOperandFlags` only
  considers `isVolatile`; ignores `!nontemporal` and notes
  "FIXME: Not preserving dereferenceable" on line 2843. This is the
  generic-target equivalent of the bug.

## Recommended fix

Pass `I.getAlign()` (already available on both `AtomicCmpXchgInst` and
`AtomicRMWInst`) instead of `DAG.getEVTAlign(MemVT)`, and pass
`I.getAAMetadata()` instead of an empty `AAMDNodes()`. These are
trivial one-line changes that bring both builders in line with
`visitLoad` / `visitStore`. The `getAtomicMemOperandFlags` FIXME
should also be addressed in the same change so that `!nontemporal`
and `MD_dereferenceable` survive.
