# w96: GVNHoist of !alias.scope unions scopes, claiming membership the original loads never had

## Affected pass
`llvm/lib/Transforms/Scalar/GVNHoist.cpp` (via
`combineMetadataForCSE` -> `MDNode::getMostGenericAliasScope` in
`llvm/lib/IR/Metadata.cpp:1150`).

## Root cause

For `MD_alias_scope`, `combineMetadata` in `Utils/Local.cpp:2958-2960`
invokes `MDNode::getMostGenericAliasScope(JMD, KMD)` which **intersects
domains then unions the scopes within those domains** (Metadata.cpp:1154):

```cpp
// Take the intersection of domains then union the scopes within those domains
```

That is, if both loads are in the same alias domain D, the hoisted load
ends up tagged with the **union** of their scope sets. `!alias.scope`'s
semantics (LangRef): "An instruction annotated with `!alias.scope`
declares that it does not alias any instruction annotated with the
corresponding `!noalias` listing any of its scopes". Membership in a
scope means **the optimizer is allowed to make no-alias inferences
against `!noalias` lists referencing that scope**.

After hoist, the single load now claims membership in *every* scope from
every branch. Suppose:
* `block a`: `%1 = load i32, ptr %p, !alias.scope !{scope1}`
* `block b`: `%2 = load i32, ptr %p, !alias.scope !{scope2}`
* later: a separate store `store i32 0, ptr %q, !noalias !{scope2}` that
  the compiler had not previously been able to prove disjoint from `%1`
  (only from `%2`).

Before hoisting, GVN could not assume `%1` is disjoint from that store.
After hoisting, the single load carries scopes `{scope1, scope2}`, and a
later AA query against the `!noalias !{scope2}` store will succeed -
even though that no-alias claim was only *separately* established on the
`b` path, not on the `a` path.

In essence, `combineMetadata` is using the rule "more scope membership ==
weaker no-alias claim from this load's perspective" -- which is true for
*this load's individual queries* against `!noalias` lists, but is
**not** the rule that captures soundness across branches. Across
branches, the correct combine is *intersection* (the same rule used for
`!noalias`, see `MDNode::intersect`): the merged access has only the
no-alias guarantees that BOTH source accesses provably had.

## Reduced reproducer

`/tmp/w96-hoist1.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i1 %c, ptr %p) {
entry:
  br i1 %c, label %a, label %b

a:
  %1 = load i32, ptr %p, align 4, !alias.scope !0
  %r1 = add i32 %1, 1
  br label %end

b:
  %2 = load i32, ptr %p, align 4, !alias.scope !1
  %r2 = add i32 %2, 1
  br label %end

end:
  %r = phi i32 [%r1, %a], [%r2, %b]
  ret i32 %r
}

!0 = !{!2}
!1 = !{!3}
!2 = !{!"scope1", !4}
!3 = !{!"scope2", !4}
!4 = !{!"domain"}
```

## opt diff

```
$ build/llvm-fuzzer/bin/opt -passes=gvn-hoist -S /tmp/w96-hoist1.ll
```

After:

```llvm
entry:
  %0 = load i32, ptr %p, align 4, !alias.scope !0
  %r1 = add i32 %0, 1
  br i1 %c, label %a, label %b
...
!0 = !{!1, !3}      ; <-- UNION of {scope1, scope2}
!1 = !{!"scope1", !2}
!2 = !{!"domain"}
!3 = !{!"scope2", !2}
```

The hoisted load asserts membership in BOTH `scope1` and `scope2`, even
though the original branch-`a` load only asserted `scope1` and the
branch-`b` load only asserted `scope2`. A subsequent `!noalias !{scope1}`
*or* `!noalias !{scope2}` access can now be proved disjoint from this
single load, whereas before the hoist only ONE of those noalias claims
held on each path.

## Manifestation (sketch)

Pair this hoist with a later memdep or BasicAA-driven elimination that
hoisted/sunk a store across the (now-merged) load using a `!noalias`
scope that was originally only valid on one path. Two passes that
consume `!alias.scope` (e.g. `gvn`, `licm`, `loop-vectorize`) suffice to
turn the metadata corruption into observable wrong code.

## Fix

`getMostGenericAliasScope` is the right name for "merge as the result of
*one access*"; it is not the right combiner when merging *two distinct
accesses on different paths* into a single instruction. GVNHoist (and
similar transforms) should use intersection-of-scopes (analogous to the
`MD_noalias` path that already uses `MDNode::intersect`) when merging
loads/stores across branches. A direct fix is to add a new
`combineForCrossBranchCSE` variant or pass an explicit "merge across
control flow" flag to `combineMetadata` that switches `MD_alias_scope` to
intersection.
