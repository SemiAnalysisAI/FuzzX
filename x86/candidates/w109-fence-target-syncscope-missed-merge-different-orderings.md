file: llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:4356-4380

`visitFenceInst` merges adjacent fences. Two paths:

1. Line 4360-4361: `FI.isIdenticalTo(NFI)` -> erase the earlier
   (handles identical fences with any syncscope including target-defined).
2. Line 4364-4379: `isIdenticalOrStrongerFence` -> erase the weaker.
   But this helper requires:

```cpp
if (FI1SyncScope != FI2->getSyncScopeID() ||
    (FI1SyncScope != SyncScope::System &&
     FI1SyncScope != SyncScope::SingleThread))
  return false;
```

So merging two fences with the *same* target-defined syncscope but
*different* orderings is rejected, even though for any sane syncscope
the stronger ordering subsumes the weaker (this is the whole point of
the partial-order on AtomicOrdering, which is syncscope-independent in
LLVM's model).

Concrete IR (opt -passes=instcombine):

  ; INPUT
  define void @f() {
    fence syncscope("agent") seq_cst
    fence syncscope("agent") acquire     ; weaker, same scope
    ret void
  }

  ; OUTPUT (unchanged, both kept)
  define void @f() {
    fence syncscope("agent") seq_cst
    fence syncscope("agent") acquire
    ret void
  }

Compare with the System-syncscope equivalent which IS merged:

  ; INPUT
  define void @g() {
    fence seq_cst
    fence acquire
    ret void
  }
  ; OUTPUT
  define void @g() {
    fence seq_cst
    ret void
  }

The TODO comment at line 4359 already flags this conservatism as
"Can remove if does not matter in practice."

Impact: missed-opt only. AMDGPU codegen for the dropped acquire
fence still emits an additional barrier (e.g., a wait-vmcnt or
buffer_gl0_inv on GFX10+), so the user pays runtime cost for a fence
that the IR-level model says is redundant.

Fix candidates:
  a) Drop the `FI1SyncScope != System && FI1SyncScope != SingleThread`
     guard entirely. The AtomicOrdering partial-order applies in any
     syncscope by IR semantics.
  b) Or, more conservatively, also accept arbitrary-but-equal syncscopes
     when one ordering strictly subsumes the other.

Note: the comment's hesitation may be that some future "weird" syncscope
could redefine ordering, but the LLVM IR memory model does not permit
that. SyncScopeID only narrows the *set of threads* a fence orders
with, not the *strength* of the ordering relation.
