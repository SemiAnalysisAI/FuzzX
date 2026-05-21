# w512 - `SelectionDAGBuilder::visitAtomicLoad` / `visitAtomicStore` drop ALL AAMD metadata (`!tbaa`, `!alias.scope`, `!noalias`) on the MMO

## Location

`llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp`:

- `SelectionDAGBuilder::visitAtomicLoad` (5312-5346), MMO built at
  line 5330-5332:
  ```c++
  MachineMemOperand *MMO = DAG.getMachineFunction().getMachineMemOperand(
      MachinePointerInfo(I.getPointerOperand()), Flags, MemVT.getStoreSize(),
      I.getAlign(), AAMDNodes(), Ranges, SSID, Order);
                    ^^^^^^^^^^^^
  ```
- `SelectionDAGBuilder::visitAtomicStore` (5348-5381), MMO built at
  line 5367-5369:
  ```c++
  MachineMemOperand *MMO = MF.getMachineMemOperand(
      MachinePointerInfo(I.getPointerOperand()), Flags, MemVT.getStoreSize(),
      I.getAlign(), AAMDNodes(), nullptr, SSID, Ordering);
                    ^^^^^^^^^^^^
  ```

The empty `AAMDNodes()` parameter discards everything the user attached:
`!tbaa`, `!tbaa.struct`, `!alias.scope`, `!noalias`. The non-atomic
counterparts (`visitLoad` 4778, `visitStore` 4934) correctly pass
`I.getAAMetadata()` via the local `AAInfo` variable.

## Repro

`atomic_ls_aa.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"

define i32 @aload(ptr %p) {
  %v = load atomic i32, ptr %p seq_cst, align 4, !tbaa !0, !alias.scope !4, !noalias !7
  ret i32 %v
}

define void @astore(ptr %p, i32 %v) {
  store atomic i32 %v, ptr %p seq_cst, align 4, !tbaa !0, !alias.scope !4, !noalias !7
  ret void
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C/C++ TBAA"}
!4 = !{!5}
!5 = distinct !{!5, !6}
!6 = distinct !{!6, !"scope-root"}
!7 = !{!8}
!8 = distinct !{!8, !6}
```

## Invocation

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel atomic_ls_aa.ll
```

## Observed

```
%v = load atomic i32, ptr %p seq_cst, align 4, !tbaa !0, !alias.scope !4, !noalias !7
  -> MOV32rm ... :: (load seq_cst (s32) from %ir.p)
                                                    ^ no !tbaa / !alias.scope / !noalias

store atomic i32 %v, ptr %p seq_cst, align 4, !tbaa !0, !alias.scope !4, !noalias !7
  -> MOV32mr ... :: (store seq_cst (s32) into %ir.p)
                                                    ^ all metadata gone
```

For comparison, an ordinary (non-atomic) load/store with the same
metadata produces a fully decorated MMO:

```
load i32, ptr %p, align 4, !tbaa !0, !alias.scope !4, !noalias !7
  -> MOV32rm ... :: (load (s32) from %ir.p, !tbaa !0, !alias.scope !4, !noalias !7)
```

## Why it matters

`MachineMemOperand::getAAInfo()` is consulted by every alias-sensitive
post-isel pass: `MachineInstr::mayAlias`, the scheduler, MachineLICM,
MachineSink, etc. Dropping the metadata means TBAA / alias.scope /
noalias proofs that the user encoded in IR are silently nullified
when the access crosses the atomic boundary - even though the
ATOMICITY itself does not block AA at the MIR layer (the
`MachineMemOperand` ordering/SSID is tracked separately from the
AAMD).

Concretely:

- A program with several `seq_cst` atomic counters in disjoint
  structures, distinguished via TBAA, will have post-isel scheduling
  treat all atomic ops as mutually aliasing, blocking otherwise-legal
  reordering.
- `noalias`-protected atomic accesses in C++ (e.g. an atomic field
  inside a `restrict`-qualified pointer's pointee) lose their proof
  exactly when the user expects the codegen to exploit it.

## Where the data is lost

- `SelectionDAGBuilder.cpp:5332` (atomic load): `AAMDNodes()` literal.
- `SelectionDAGBuilder.cpp:5369` (atomic store): `AAMDNodes()` literal.
- `SelectionDAGBuilder.cpp:5213` (cmpxchg) and `5285` (atomicrmw)
  have the same bug (separately filed as w511, which is also
  alignment-related).

## Recommended fix

Replace the literal `AAMDNodes()` with `I.getAAMetadata()` in both
builders. The non-atomic builders already do this; the change brings
visitAtomicLoad / visitAtomicStore in line. This is a strict
strict improvement: no information is added that wasn't already in
the IR, and the AAInfo fields are independent of the atomic ordering
machinery already plumbed through.
