# w97: SROA speculatePHINodeLoads picks AAMetadata/alignment from one arbitrary user, applies to all-load replacement

## Component
`llvm/lib/Transforms/Scalar/SROA.cpp` — `speculatePHINodeLoads` (around line 1624)

## Root cause
When SROA speculates loads through a PHI of pointers, it converts:
```
%p = phi ptr [ %ptr1, %L ], [ %ptr2, %R ]
%vA = load T, ptr %p, !tbaa !X     ; user A
%vB = load T, ptr %p, !tbaa !Y     ; user B (different tbaa)
```
into one PHI of values with one load per pred-block.

In `speculatePHINodeLoads` (SROA.cpp ~line 1633):
```cpp
// Get the AA tags and alignment to use from one of the loads.
// It does not matter which one we get and if any differ.
AAMDNodes AATags = SomeLoad->getAAMetadata();
Align Alignment = SomeLoad->getAlign();
```
`SomeLoad = cast<LoadInst>(PN.user_back())` — just one arbitrary user.

Then **both** of the original loads (A and B) are RAUW'd to the new PHI; the injected
predecessor loads only carry `AATags` from one user (B), but the result feeds *all*
users that originally had different !tbaa. The "if any differ" comment is wrong — it
matters for AA-based reasoning downstream.

A subsequent TBAA-based pass (GVN/MemDep) sees only `!tbaa !Y` on the speculated load
and may conclude no-alias with a store whose !tbaa was compatible with `!X` but not `!Y`
— silently deleting/forwarding wrongly.

## opt diff (reproducible with `opt -passes=sroa -S`)
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @sink_i(i64)
declare void @sink_f(i64)

define void @test(i1 %c, ptr %ext) {
entry:
  %a = alloca i64, align 8
  store i64 100, ptr %a, align 8
  br i1 %c, label %L, label %R
L:
  br label %J
R:
  br label %J
J:
  %p = phi ptr [ %a, %L ], [ %ext, %R ]
  %vi = load i64, ptr %p, align 8, !tbaa !2     ; "int" load
  %vf = load i64, ptr %p, align 8, !tbaa !4     ; "float" load
  call void @sink_i(i64 %vi)
  call void @sink_f(i64 %vf)
  ret void
}

!0 = !{!"root"}
!1 = !{!"int", !0, i64 0}
!2 = !{!1, !1, i64 0}
!3 = !{!"float", !0, i64 0}
!4 = !{!3, !3, i64 0}
```
Output:
```ll
R:
  %p.sroa.speculate.load.R = load i64, ptr %ext, align 8, !tbaa !0  ; !0 = "float"
  br label %J
J:
  %p.sroa.speculated = phi i64 [ 100, %L ], [ %p.sroa.speculate.load.R, %R ]
  call void @sink_i(i64 %p.sroa.speculated)   ; was !tbaa "int"
  call void @sink_f(i64 %p.sroa.speculated)   ; was !tbaa "float"
```
The injected load carries `!tbaa "float"` only. Both `%vi` (originally "int") and
`%vf` (originally "float") are now sourced from a single SSA value with one tag.
The "int" annotation is **gone**, weakening AA in one direction and strengthening
it (incorrectly!) in another, depending on which user picks `user_back()`.

## Worse variant (alignment)
If the alloca's load was `align 8` and the other-pointer load was `align 1`,
`SomeLoad = user_back()` may pick the align-1 version. The injected loads then
have `align 1` — overly conservative but not unsafe. Reverse it (alloca's load
align 1, ext load align 8, pick user_back = align 8) and the injected
`load i64, ptr %ext, align 8` may be **misaligned UB** if `%ext` is align-1.

## Fix sketch
Walk *all* of the PHI's load users and intersect AA tags (intersection of `!tbaa`,
`!alias.scope`, `!noalias`) and pick the **minimum** alignment, not arbitrary
last-user values.

## Notes
- Distinct from w78-sroa-{tree-merge,vector-promotion}-drops-atomic. This is AA/align
  loss on PHI-pointer speculation, not atomic-flag drop.
- Repro on LLVM 23.0.0git (FuzzX `opt` build).
