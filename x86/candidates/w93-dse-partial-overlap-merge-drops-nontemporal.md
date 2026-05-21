file: llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp:2683-2708
(partial-store-merging branch in eliminateDeadStores)

When `OR == OW_PartialEarlierWithFullLater` and partial-store-merging
is enabled (default), DSE folds the killing store's bytes into the
dead store via `tryToMergePartialOverlappingStores`. The dead store's
stored constant is updated with `DeadSI->setOperand(0, Merged)` and
the killing store is removed with `deleteDeadInstruction(KillingSI)`.

The killing store's metadata is discarded entirely. If the killing
store carries `!nontemporal`, the user's nontemporal hint for that
slice of memory is silently dropped from the merged store.

Reproducer:

  target triple = "x86_64-unknown-linux-gnu"

  define void @f(ptr %p) {
  entry:
    store i64 0, ptr %p, align 8                       ; dead
    store i32 -1, ptr %p, align 4, !nontemporal !0     ; killing (nontemporal)
    ret void
  }

  !0 = !{i32 1}

opt -passes=dse output:

  define void @f(ptr %p) {
  entry:
    store i64 4294967295, ptr %p, align 8
    ret void
  }

llc -mtriple=x86_64-- -mattr=+sse4.1 codegen diff:

  Without DSE:
    movq    $0, (%rdi)
    movl    $-1, %eax
    movntil %eax, (%rdi)        ; nontemporal write of bytes 0..3
  With DSE:
    movl    $4294967295, %eax
    movq    %rax, (%rdi)        ; plain temporal write, no nontemporal

The merged 8-byte temporal store is observably different: the
low 4 bytes were requested as nontemporal (write-combining /
cache-bypass), but the resulting code uses a regular store.

Fix: before performing the merge, refuse if the killing store has
metadata that cannot survive being attached to the (wider) dead
store unchanged, in particular MD_nontemporal. Alternatively,
propagate MD_nontemporal to DeadSI when KillingSI has it (and
require all merged-in killing stores to agree). Same care is needed
for MD_invariant_group, MD_alias_scope/noalias, and MD_tbaa.
