## RS4GC preserves `!noalias` / `!alias.scope` / `!invariant.load` on `atomicrmw` and `cmpxchg`

`llvm/lib/Transforms/Scalar/RewriteStatepointsForGC.cpp:2920-2945` (`stripInvalidMetadataFromInstruction`)

```cpp
static void stripInvalidMetadataFromInstruction(Instruction &I) {
  if (!isa<LoadInst>(I) && !isa<StoreInst>(I))
    return;
  // ...
  // We also drop the invariant.load metadata on the load because that metadata
  // implies the address operand to the load points to memory that is never
  // changed once it became dereferenceable. This is no longer true after RS4GC.
  // ...
  unsigned ValidMetadataAfterRS4GC[] = {LLVMContext::MD_tbaa, ...};
  I.dropUnknownNonDebugMetadata(ValidMetadataAfterRS4GC);
}
```

The function bails out for everything that is not a `LoadInst` or `StoreInst`,
so an `AtomicRMWInst` or `AtomicCmpXchgInst` keeps `!noalias`, `!alias.scope`,
`!dereferenceable`, `!invariant.load`, etc. across the pass.

The comment at lines 2925-2933 explicitly states the rationale ("gc.statepoint
can touch the entire heap including noalias objects" and "invariant.load ...
is no longer true after RS4GC"). The *exact same reasoning* applies to atomic
RMW and cmpxchg, which carry a load+store memory access whose pre-statepoint
properties (no-alias scope, invariance) are invalidated by the statepoint
clobbering the heap. The IR verifier accepts all of these attachments on
`atomicrmw`/`cmpxchg`, so they happily reach the post-RS4GC pipeline (LICM,
GVN, MemorySSA-based passes) and can be used to incorrectly sink or fold
across the statepoint.

### Candidate IR

```
target triple = "x86_64-unknown-linux-gnu"

declare void @bar()
declare token @llvm.experimental.gc.statepoint.p0(i64, i32, ptr, i32, i32, ...)

define i64 @test_arr(ptr addrspace(1) %p) gc "statepoint-example" {
  %v  = atomicrmw add ptr addrspace(1) %p, i64 1 seq_cst, !invariant.load !0, !noalias !2
  call void @bar()
  %v2 = atomicrmw add ptr addrspace(1) %p, i64 1 seq_cst, !invariant.load !0, !noalias !2
  ret i64 %v2
}

define i64 @test_cas(ptr addrspace(1) %p) gc "statepoint-example" {
  %v  = cmpxchg ptr addrspace(1) %p, i64 0, i64 1 seq_cst seq_cst, !alias.scope !2, !noalias !2
  %r  = extractvalue { i64, i1 } %v, 0
  call void @bar()
  %v2 = cmpxchg ptr addrspace(1) %p, i64 0, i64 1 seq_cst seq_cst, !alias.scope !2, !noalias !2
  %r2 = extractvalue { i64, i1 } %v2, 0
  ret i64 %r2
}

!0 = !{}
!1 = distinct !{!1, !3, !"scope"}
!2 = !{!1}
!3 = distinct !{!3, !"domain"}
```

### Observed (wrong) output

`opt -passes=rewrite-statepoints-for-gc -S`:

```
define i64 @test_arr(ptr addrspace(1) %p) gc "statepoint-example" {
  %v = atomicrmw add ptr addrspace(1) %p, i64 1 seq_cst, align 8,
                      !invariant.load !0, !noalias !1               ; <-- BOTH preserved
  %statepoint_token = call token (...) @llvm.experimental.gc.statepoint...
  %p.relocated      = call ... @llvm.experimental.gc.relocate.p1(...)
  %v2 = atomicrmw add ptr addrspace(1) %p.relocated, i64 1 seq_cst, align 8,
                      !invariant.load !0, !noalias !1               ; <-- BOTH preserved
  ret i64 %v2
}

define i64 @test_cas(ptr addrspace(1) %p) gc "statepoint-example" {
  %v  = cmpxchg ptr addrspace(1) %p, i64 0, i64 1 seq_cst seq_cst, align 8,
                  !alias.scope !1, !noalias !1                      ; <-- BOTH preserved
  ...
  %v2 = cmpxchg ptr addrspace(1) %p.relocated, i64 0, i64 1 seq_cst seq_cst, align 8,
                  !alias.scope !1, !noalias !1                      ; <-- BOTH preserved
}
```

Compare with the analogous `load` form, where the same metadata is dropped:

```
%v = load i64, ptr addrspace(1) %p, align 8                 ; no !noalias, no !invariant.load
```

### Expected wrong outcome

Post-RS4GC, a MemorySSA-driven pass (e.g. LICM hoist/sink, EarlyCSE,
DSE) can use the stale `!noalias` / `!alias.scope` to conclude the two
`atomicrmw add` operations do not alias with whatever escape path the
`@bar()` statepoint might touch. That permits reordering, common-subexpression
folding, or hoisting of the second `atomicrmw` across the statepoint — exactly
the class of miscompile that the existing `drop-invalid-metadata.ll` regression
test was added to prevent for plain loads/stores.

A clean fix is to drop the early-return at line 2921-2922 (or extend it to
`isa<AtomicRMWInst>(I) || isa<AtomicCmpXchgInst>(I)`) and let
`dropUnknownNonDebugMetadata` strip the same allow-listed set on atomic memory
ops.

### Reproducers

* `/tmp/rs4gc_test/t_atomicrmw_noalias.ll`
* `/tmp/rs4gc_test/t_arrmw_invload.ll`
* `/tmp/rs4gc_test/t_cmpxchg_noalias.ll`
