# w611: LoopDistribute / LoopVersioning - asymmetric / incomplete `!noalias` annotation across partitions

- **Pass**: `llvm/lib/Transforms/Scalar/LoopDistribute.cpp` +
  `llvm/lib/Transforms/Utils/LoopVersioning.cpp`
- **Gating**: `loop-distribute` runs in default `-O2`
  (`llvm/lib/Passes/PassBuilderPipelines.cpp:1608`) BUT only triggers when the
  loop carries `!llvm.loop.distribute.enable` metadata (LoopDistribute.cpp:942)
  OR with `--enable-loop-distribute`.
- **Severity**: Missed optimization (incomplete `!noalias` annotations on
  distributed loops). Not a miscompile — annotations are always sound when
  present — but produces strictly suboptimal alias metadata for the runtime
  pointer checks that the user paid for. Cross-partition reordering /
  vectorization opportunities are silently lost on a subset of accesses.

## Root cause

`LoopVersioning::prepareNoAliasMetadata` (LoopVersioning.cpp:178-217)
processes the runtime alias checks one-directionally:

```cpp
for (const auto &Check : AliasChecks)
  GroupToNonAliasingScopes[Check.first].push_back(GroupToScope[Check.second]);
```

Each runtime check is a *pair* `(grp_a, grp_b)` produced by
`LoopAccessAnalysis`; the order is not symmetric and there is normally only
ONE direction stored per cross-checked pair. The loop above only inserts a
no-alias scope on `Check.first`'s entry — `Check.second` does not learn that
it does not alias `Check.first`.

`LoopDistribute` adds to this by calling `Partitions.cloneLoops()`
(LoopDistribute.cpp:836) AFTER `LVer.annotateLoopWithNoAlias()`
(LoopDistribute.cpp:820). Each partition (except the last) is a *clone* of
the already-annotated versioned loop, then `removeUnusedInsts` strips
non-partition instructions. The result: the LAST partition (which is the
original versioned loop with the per-partition unused instructions removed)
keeps the same annotations the original instructions got — which, due to the
one-directional `Check.first` issue, can be empty / single-scope, while
instructions in CLONED partitions might have richer scopes purely because
their pointer group happened to be the `Check.first` side more often.

## Reproducer

`/tmp/w610/dist3.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@A = common global ptr null, align 8
@B = common global ptr null, align 8
@C = common global ptr null, align 8
@D = common global ptr null, align 8
@E = common global ptr null, align 8

define void @f() {
entry:
  %a = load ptr, ptr @A, align 8
  %b = load ptr, ptr @B, align 8
  %c = load ptr, ptr @C, align 8
  %d = load ptr, ptr @D, align 8
  %e = load ptr, ptr @E, align 8
  br label %for.body

for.body:
  %ind = phi i64 [ 0, %entry ], [ %add, %for.body ]
  %arrayidxA = getelementptr inbounds i32, ptr %a, i64 %ind
  %loadA = load i32, ptr %arrayidxA, align 4
  %arrayidxB = getelementptr inbounds i32, ptr %b, i64 %ind
  %loadB = load i32, ptr %arrayidxB, align 4
  %mulA = mul i32 %loadB, %loadA
  %add = add nuw nsw i64 %ind, 1
  %arrayidxA_plus_4 = getelementptr inbounds i32, ptr %a, i64 %add
  store i32 %mulA, ptr %arrayidxA_plus_4, align 4
  %arrayidxD = getelementptr inbounds i32, ptr %d, i64 %ind
  %loadD = load i32, ptr %arrayidxD, align 4
  %arrayidxE = getelementptr inbounds i32, ptr %e, i64 %ind
  %loadE = load i32, ptr %arrayidxE, align 4
  %mulC = mul i32 %loadD, %loadE
  %arrayidxC = getelementptr inbounds i32, ptr %c, i64 %ind
  store i32 %mulC, ptr %arrayidxC, align 4
  %exitcond = icmp eq i64 %add, 20
  br i1 %exitcond, label %for.end, label %for.body

for.end:
  ret void
}
```

Command:

```bash
opt -aa-pipeline=basic-aa -S -passes='loop-distribute' --enable-loop-distribute /tmp/w610/dist3.ll
```

## Observed asymmetry

After distribution there are two body loops (`for.body.ldist1` is partition 1
with the A/B stream, `for.body` (the *original* loop, now last partition) has
the C/D/E stream). The cross-partition scopes are `!15 (=C grp), !16 (=D
grp), !17 (=E grp), !12 (=A grp), !19 (=B grp)`.

Partition 1 (cloned) loads/stores:

| inst   | alias.scope        | noalias                          |
|--------|--------------------|----------------------------------|
| loadA  | `{orig_A, !12}`    | `{orig_notA, !15, !16, !17}`    |
| loadB  | `{!19}`            | (none)                           |
| storeA | `{!12}`            | `{!15, !16, !17}`                |

Partition 2 (original, last):

| inst   | alias.scope     | noalias            |
|--------|-----------------|--------------------|
| loadD  | `{orig_D, !16}` | `{orig_notD}`      |
| loadE  | `{!17}`         | (none)             |
| storeC | `{!15}`         | `{!19}` only       |

`storeC` SHOULD carry `noalias` with `{!12 (A), !19 (B), !16 (D), !17 (E)}`
to fully exploit the runtime checks, but only has `{!19}`. `loadD` lacks
cross-partition noalias entirely. `loadE` has no noalias annotation. After
distribution the second partition can no longer prove that, e.g., its
`storeC` doesn't alias `loadA` in the first partition by reading metadata
alone.

## Impact

Downstream passes (LICM, GVN, DSE, LoopVectorize) that consult `!alias.scope`
/ `!noalias` get full information for half the accesses and incomplete
information for the rest. With identical IR but a different pointer-ordering
(or a different traversal of the runtime checks), the OPPOSITE partition can
end up with the incomplete annotations, producing non-deterministic
optimization quality across structurally equivalent inputs.

## Suggested fix sketch

In `LoopVersioning::prepareNoAliasMetadata` make the noalias relation
symmetric:

```cpp
for (const auto &Check : AliasChecks) {
  GroupToNonAliasingScopes[Check.first].push_back(GroupToScope[Check.second]);
  GroupToNonAliasingScopes[Check.second].push_back(GroupToScope[Check.first]);
}
```

The relation "groups don't alias" is inherently symmetric — the existing
asymmetry appears to be a bug, not a deliberate design choice.
