# w534: LowerGuard drops `!noalias`/`!alias.scope`/non-`make_implicit` metadata when explicating the guard

## Summary
`makeGuardControlFlowExplicit` (used by the `lower-guard-intrinsic` pass)
only forwards a single metadata kind - `MD_make_implicit` - from the
original `@llvm.experimental.guard` call to the new check branch. All other
metadata attached to the guard call is silently dropped: notably `!noalias`,
`!alias.scope`, `!nosanitize`, `!callback`, and any custom analysis
metadata.

The hottest plausible loss is `!noalias`/`!alias.scope`: a frontend that
emits a guard with scoped-alias metadata to convey aliasing facts about the
guarded region loses those facts on the explicit control-flow form. Note
that this is *not* what the brief originally hypothesised (loss of
`!alias.scope` on the *next* instruction); the next instruction's metadata
is preserved by `SplitBlockAndInsertIfThen`. The real loss is on the guard
itself, which after lowering disappears entirely.

## Source
File: `llvm/lib/Transforms/Utils/GuardUtils.cpp`

```cpp
// lines 48-53
if (auto *MD = Guard->getMetadata(LLVMContext::MD_make_implicit))
  CheckBI->setMetadata(LLVMContext::MD_make_implicit, MD);

MDBuilder MDB(Guard->getContext());
CheckBI->setMetadata(LLVMContext::MD_prof,
                     MDB.createBranchWeights(PredicatePassBranchWeight, 1));
```

`Guard->getAllMetadata(...)` is never queried. The deopt call inserted into
the new `deopt` block (lines 56-65) also does not receive any forwarded
metadata from the original guard.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @f(i1 %c, ptr %p, ptr %q) {
entry:
  call void (i1, ...) @llvm.experimental.guard(i1 %c) [ "deopt"() ], !noalias !0
  store i32 1, ptr %p, !alias.scope !0
  store i32 2, ptr %q
  ret void
}
declare void @llvm.experimental.guard(i1, ...)

!0 = !{!1}
!1 = distinct !{!1, !2}
!2 = distinct !{!2}
```

Run:
```
opt -passes=lower-guard-intrinsic -S
```

## Observed
```
entry:
  br i1 %c, label %guarded, label %deopt, !prof !0
deopt:
  call void (...) @llvm.experimental.deoptimize.isVoid() [ "deopt"() ]
  ret void
guarded:
  store i32 1, ptr %p, align 4, !alias.scope !1
  store i32 2, ptr %q, align 4
  ret void
}
```

The `!noalias !0` on the guard is gone; neither the new `br` nor the new
`call llvm.experimental.deoptimize` carries it. The downstream `store ...,
!alias.scope !0` keeps its annotation because its instruction is untouched.

## Impact
A frontend or earlier pass that adds `!noalias`/`!alias.scope` to a guard
to inform an alias-set analysis loses those facts at the
`lower-guard-intrinsic` pass. After lowering, downstream alias analyses see
the deopt call as having no scoped-alias relationship, potentially missing
opportunities for noalias-based loop-invariant code motion etc.

This is the same class of bug as the well-known "RAUW preserves uses but
not metadata"; the fix is to copy the guard's metadata onto the new branch
and/or the new deopt call. The existing single-kind forward
(`MD_make_implicit`) is a partial implementation.

## Default-pipeline confirmation
`lower-guard-intrinsic` is not part of the default `-O2` pass pipeline; it
is invoked by toolchains using LLVM's experimental guard mechanism (e.g.,
some JITs). For a default-x86-O2-only repro a synthetic invocation of the
pass on its own is required. Listed for completeness of the focus areas;
ranked lower than w530-w533.
