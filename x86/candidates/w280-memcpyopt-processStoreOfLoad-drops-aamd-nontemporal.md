# MemCpyOpt processStoreOfLoad drops AAMD, !nontemporal, and !invariant.load from load/store

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::processStoreOfLoad`

**Lines:** 631-702 (the load+store -> memcpy/memmove transform). Metadata copy at line 685.

## Bug

When a `load %T; store %T` aggregate pair is replaced with `memcpy`/`memmove`,
the only metadata propagated from the store to the new intrinsic is
`MD_DIAssignID`:

```cpp
// line 685
M->copyMetadata(*SI, LLVMContext::MD_DIAssignID);
```

There is no `combineAAMetadata(M, LI)` or `combineAAMetadata(M, SI)` call, and
no propagation of `LLVMContext::MD_nontemporal` or
`LLVMContext::MD_invariant_load` from the load. The new memcpy/memmove is
therefore stripped of:

- `!alias.scope`, `!noalias`, `!tbaa`, `!tbaa.struct` on the store
- `!alias.scope`, `!noalias`, `!tbaa`, `!invariant.load`, `!invariant.group`,
  `!nontemporal`, `!noundef` on the load

Contrast with `processMemCpyMemCpyDependence` (line 1256, same file), which
also misses AAMD propagation, or `processByValArgument` (line 2070) which
*does* call `combineAAMetadata(&CB, MDep)`. The right thing is to merge AAMD
from both load and store into the new memcpy/memmove (combineAAMetadata of
load+store) and copy `!nontemporal` from the *load*. The store's
`!nontemporal` is filtered upstream at line 755 inside `processStore`, but
the load's is silently dropped.

Loss of `!alias.scope`/`!noalias` is a correctness regression for downstream
AA: it widens the AA set, which can defeat noalias-based optimizations that
the front end carefully built up. Loss of `!nontemporal` defeats explicit
hardware-cache hints set by the user (e.g. via `__builtin_nontemporal_load`),
forcing the backend to emit ordinary moves instead of MOVNT-class loads.

## Confirmed via opt

### Case A: `!alias.scope` on the store dropped

```ll
target triple = "x86_64-unknown-linux-gnu"

%S = type { i64, i64, i64, i64 }

declare void @use(ptr)

define void @test(ptr noalias %dst, ptr noalias %src) {
  %x = load %S, ptr %src, align 8
  store %S %x, ptr %dst, align 8, !alias.scope !1
  call void @use(ptr %dst)
  ret void
}

!1 = !{!2}
!2 = distinct !{!2, !3, !"scope"}
!3 = distinct !{!3, !"domain"}
```

After `opt -passes=memcpyopt -S`:

```ll
define void @test(ptr noalias %dst, ptr noalias %src) {
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src,
                                   i64 32, i1 false)   ; no !alias.scope
  call void @use(ptr %dst)
  ret void
}
```

### Case B: `!nontemporal` on the load dropped

```ll
%S = type { i64, i64, i64, i64 }

declare void @use(ptr)

define void @test(ptr noalias %dst, ptr noalias %src) {
  %x = load %S, ptr %src, align 8, !nontemporal !0
  store %S %x, ptr %dst, align 8
  call void @use(ptr %dst)
  ret void
}

!0 = !{i32 1}
```

After `opt -passes=memcpyopt -S`:

```ll
define void @test(ptr noalias %dst, ptr noalias %src) {
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src,
                                   i64 32, i1 false)   ; no !nontemporal
  ...
}
```

### Case C: `!invariant.load` on the load dropped

Same scaffolding with `!invariant.load !0` on the load yields a memcpy with
no metadata at all.

## Fix sketch

Replace line 685 with:

```cpp
M->copyMetadata(*SI, LLVMContext::MD_DIAssignID);
combineAAMetadata(M, LI);
combineAAMetadata(M, SI);
if (LI->getMetadata(LLVMContext::MD_nontemporal))
  M->setMetadata(LLVMContext::MD_nontemporal,
                 LI->getMetadata(LLVMContext::MD_nontemporal));
```

`processStore` already bails on store-side `!nontemporal` at line 755.
