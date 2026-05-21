# w655: mergeicmps drops `!tbaa` (and other load AAMD) when merging into memcmp

- **File:** `llvm/lib/Transforms/Scalar/MergeICmps.cpp`
- **Target:** x86_64, LLVM 23.0.0git, default `-O2` lowering path
- **Pass:** `mergeicmps`
- **Severity:** Missed optimization / AA pessimization (potential correctness regression downstream if dropped metadata had been load-only and a later pass relies on TBAA disjointness)

## Root cause

`mergeComparisons()` builds the merged memcmp call without copying any AA
metadata (`!tbaa`, `!tbaa.struct`, `!alias.scope`, `!noalias`,
`!invariant.load`, `!nontemporal`, ...) from the source loads.

`MergeICmps.cpp:680-707`:

```cpp
  if (Comparisons.size() == 1) {
    // Use clone to keep the metadata
    Instruction *const LhsLoad = Builder.Insert(FirstCmp.Lhs().LoadI->clone());
    Instruction *const RhsLoad = Builder.Insert(FirstCmp.Rhs().LoadI->clone());
    ...
    IsEqual = Builder.CreateICmpEQ(LhsLoad, RhsLoad);
  } else {
    ...
    Value *const MemCmpCall = emitMemCmp(             // <-- no AAMDNodes / metadata
        Lhs, Rhs,
        ConstantInt::get(Builder.getIntNTy(SizeTBits), TotalSizeBits / 8),
        Builder, DL, &TLI);
    ...
  }
```

Note that the single-comparison path (line 681 comment `Use clone to keep
the metadata`) explicitly *intends* to keep metadata via `clone()`, but
the multi-comparison path that synthesizes a fresh `memcmp` call never
calls `MergedCall->setAAMetadata(...)` / `setMetadata(...)`. There is no
union of the source loads' AAMDNodes.

## Repro

`/tmp/mergeicmps/tbaa_drop.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

define i1 @eq(ptr dereferenceable(8) %a, ptr dereferenceable(8) %b) {
entry:
  %a0 = load i32, ptr %a, align 4, !tbaa !2
  %b0 = load i32, ptr %b, align 4, !tbaa !2
  %c0 = icmp eq i32 %a0, %b0
  br i1 %c0, label %land.rhs, label %land.end

land.rhs:
  %ag1 = getelementptr inbounds %S, ptr %a, i64 0, i32 1
  %bg1 = getelementptr inbounds %S, ptr %b, i64 0, i32 1
  %a1 = load i32, ptr %ag1, align 4, !tbaa !6
  %b1 = load i32, ptr %bg1, align 4, !tbaa !6
  %c1 = icmp eq i32 %a1, %b1
  br label %land.end

land.end:
  %r = phi i1 [ false, %entry ], [ %c1, %land.rhs ]
  ret i1 %r
}

!2 = !{!3, !3, i64 0}
!3 = !{!"int", !4, i64 0}
!4 = !{!"omnipotent char", !5, i64 0}
!5 = !{!"Simple C++ TBAA"}
!6 = !{!7, !7, i64 0}
!7 = !{!"some-other-typed-int", !4, i64 0}
```

## Diff (`opt -passes=mergeicmps -S`)

```
-  %a0 = load i32, ptr %a, align 4, !tbaa !2
-  %b0 = load i32, ptr %b, align 4, !tbaa !2
-  ...
-  %a1 = load i32, ptr %ag1, align 4, !tbaa !6
-  %b1 = load i32, ptr %bg1, align 4, !tbaa !6
-  ...
+  %memcmp = call i32 @memcmp(ptr %a, ptr %b, i64 8)
+  %0 = icmp eq i32 %memcmp, 0
```

All TBAA, `align`, and other AAMDNodes are gone. The synthesized
`@memcmp` call has only the libcall attributes
(`memory(argmem: read)`); no `!tbaa`, no `!tbaa.struct`, no scopes.

## Why this matters on x86 -O2

1. ExpandMemCmpPass later turns small fixed-size memcmps into in-line
   loads + xor/cmp (this is x86-O2's normal path; see
   `CodeGen/ExpandMemCmpPass.cpp`). Those synthesized loads inherit no
   TBAA from the original chain, so post-codegen MIR / DAG sees them as
   pessimistic generic memory accesses.
2. Cross-pass MachineMemOperand population uses
   `getAAInfo()`/MMO TBAA. Losing it here can defeat machine-LICM and
   post-RA scheduler heuristics that rely on `MachineMemOperand`
   no-alias info.
3. Spec is "merge into a memcmp" but the contract of the source loads
   (e.g., `!alias.scope` set proving they don't alias another body)
   silently vanishes; a later inliner can no longer reason the merged
   call doesn't alias.

## Suggested fix

After `emitMemCmp(...)`, intersect AAMDNodes of all four source loads
and apply them to the call (and to any `!nontemporal` — see w657):

```cpp
auto *Call = cast<Instruction>(MemCmpCall);
AAMDNodes Combined = FirstCmp.Lhs().LoadI->getAAMetadata();
for (const BCECmpBlock &C : Comparisons.drop_front()) {
  Combined = Combined.merge(C.Lhs().LoadI->getAAMetadata());
  Combined = Combined.merge(C.Rhs().LoadI->getAAMetadata());
}
Combined = Combined.merge(FirstCmp.Rhs().LoadI->getAAMetadata());
Call->setAAMetadata(Combined);
```

## Status

Confirmed with `opt -passes=mergeicmps -S` on the provided repro.
Source references all from `llvm/lib/Transforms/Scalar/MergeICmps.cpp`
(LLVM main, commit at `amdgpu/third_party/llvm-project`).
