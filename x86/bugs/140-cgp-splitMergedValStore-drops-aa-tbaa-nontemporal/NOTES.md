# CGP splitMergedValStore drops !tbaa / !nontemporal / !alias.scope / !noalias on the split stores

File: `llvm/lib/CodeGen/CodeGenPrepare.cpp:8568-8662` (function
`splitMergedValStore`, helper lambda `CreateSplitStore` at 8638-8654).

## Reasoning

`splitMergedValStore` rewrites
`store i64 (or (zext lo), (shl (zext hi), 32)), ptr` into two half-width
stores. Each replacement store is created via:

```cpp
Builder.CreateAlignedStore(V, Addr, Alignment);
```

Crucially, no metadata is copied from the original `StoreInst &SI`. The
following pieces of IR-level information are silently dropped on the
resulting low/high stores:

| Metadata kind        | Effect of dropping                                    |
|----------------------|-------------------------------------------------------|
| `!tbaa`              | AA cannot disambiguate split stores from later loads of unrelated TBAA types; can re-enable invalid reordering downstream |
| `!tbaa.struct`       | Same; struct-component AA info is lost                |
| `!alias.scope`       | Stores re-enter the "may alias everything" pool       |
| `!noalias`           | Same — alias-set restriction is lost                  |
| `!nontemporal`       | **Direct codegen miscompile**: user-requested non-temporal write is downgraded to a regular cache-polluting store |
| `!annotation`        | Other passes' annotations lost                        |
| `!DIAssignID`        | Assignment tracking debug info broken                 |

The `!nontemporal` case is the most easily-observable miscompile because
the IR-level contract maps directly to a concrete x86 instruction
(`MOVNTI`). When CGP fires this pattern the user gets two regular `mov`
instructions instead of two non-temporal stores. For a write-combining /
streaming-store workload that's a real performance/semantic bug, not
just a missed optimisation.

The `!tbaa`/`!noalias` loss is a soundness bug: AA queries after CGP
will see two type-unconstrained scalar stores instead of one
type-constrained 8-byte store, which can let later passes (DAG combine,
post-RA scheduler) reorder or fold loads/stores that originally were
disambiguated.

The fix is the standard pattern used elsewhere in CGP (e.g. SROA's split
store helpers, and the matching split-load helper):

```cpp
auto *NewSt = Builder.CreateAlignedStore(V, Addr, Alignment);
NewSt->copyMetadata(SI, {LLVMContext::MD_tbaa, LLVMContext::MD_tbaa_struct,
                         LLVMContext::MD_alias_scope, LLVMContext::MD_noalias,
                         LLVMContext::MD_nontemporal, LLVMContext::MD_annotation,
                         LLVMContext::MD_DIAssignID});
```

(Or simply `NewSt->copyMetadata(SI)` after the split.)

## IR repro

Run with:

```
llc -stop-after=codegenprepare -mtriple=x86_64-unknown-linux-gnu repro.ll -o -
```

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @split_nontemporal_tbaa(ptr %p, i32 %lo, float %hf) {
entry:
  %hi.i = bitcast float %hf to i32
  %lo.z = zext i32 %lo  to i64
  %hi.z = zext i32 %hi.i to i64
  %hi.s = shl i64 %hi.z, 32
  %v    = or  i64 %lo.z, %hi.s
  store i64 %v, ptr %p, align 8, !nontemporal !0, !tbaa !1, !alias.scope !4, !noalias !6
  ret void
}

!0 = !{i32 1}
!1 = !{!2, !2, i64 0}
!2 = !{!"long long", !3}
!3 = !{!"omnipotent char"}
!4 = !{!5}
!5 = distinct !{!5, !"scope1"}
!6 = !{!7}
!7 = distinct !{!7, !"scope2"}
```

## Observed wrong outcome

After CGP the IR becomes:

```
  store i32 %lo, ptr %p, align 8
  store i32 %hi.i, ptr %0, align 4
  ret void
```

- `!nontemporal` is gone → the backend will not emit `movnti`. The
  original IR contract for streaming/non-temporal write is broken.
- `!tbaa` / `!alias.scope` / `!noalias` are gone → AA disambiguation
  between this pair of writes and unrelated later memory ops is
  silently weakened, which can enable downstream miscompiles (e.g. an
  unrelated load that was AA-distinct from `store i64` may now be
  considered `MayAlias` with the split `store i32`s, blocking valid
  reordering — or vice-versa depending on how downstream passes
  interpret the *absence* of TBAA).
- The two new stores also have no `!DIAssignID`, so assignment-tracking
  debug info that pointed at the original 8-byte store is left
  dangling.

This is independent of (and stacks with) the previously-known
atomic-bailout bug for the same function (#012 /
`w25-splitMergedValStore-atomic-not-checked.md`).
