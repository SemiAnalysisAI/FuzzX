file: llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp:2450-2507
(DSEState::eliminateRedundantStoresOfExistingValues)

When two stores write the same value to the same location ("redundant
existing value" elimination), DSE picks one store to keep and deletes
the other without performing any metadata merge or transfer. The
identity check uses `Instruction::isIdenticalToWhenDefined(...,
/*IntersectAttrs=*/true)`, which ignores instruction metadata. So a
plain `store` and `store ..., !nontemporal !0` are treated as
identical, and the lower one (the iteration's `DefInst`) is dropped
via `deleteDeadInstruction(DefInst)`. The kept store retains only its
own metadata.

This is reachable through the main `eliminateDeadStores` path too: a
later store of an identical value with !nontemporal can also be
killed as a "dead" store, losing the user's nontemporal hint.

Reproducer:

  target triple = "x86_64-unknown-linux-gnu"

  define void @f(ptr %p, i32 %v) {
  entry:
    store i32 %v, ptr %p, align 4, !nontemporal !0
    br label %next
  next:
    store i32 %v, ptr %p, align 4
    ret void
  }

  !0 = !{i32 1}

opt -passes=dse output:

  define void @f(ptr %p, i32 %v) {
  entry:
    store i32 %v, ptr %p, align 4
    ret void
  }

llc -mtriple=x86_64-- -mattr=+sse2 codegen diff:

  Without DSE:
    movntil %edx, (%rdi)    ; nontemporal write
    movl    %edx, (%rdi)
  With DSE:
    movl    %edx, (%rdi)    ; nontemporal hint LOST

Same-block reproducer also miscompiles (both upper-nontemporal-lower-
plain and upper-plain-lower-nontemporal lose `!nontemporal` in at
least one ordering, depending on which path catches it first).

Fix: in `eliminateRedundantStoresOfExistingValues`, after picking the
survivor, intersect/merge MD_nontemporal, MD_invariant_group,
MD_alias_scope, MD_noalias, MD_tbaa (similar to combineMetadata in
Local.cpp), or refuse to delete when the deleted store carries
attributes/metadata the survivor lacks. Same fix applies to the
"redundant identical store" branch in the main loop.
