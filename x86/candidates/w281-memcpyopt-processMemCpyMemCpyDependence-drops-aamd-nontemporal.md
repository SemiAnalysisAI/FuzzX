# MemCpyOpt processMemCpyMemCpyDependence drops AAMD and !nontemporal from outer memcpy

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::processMemCpyMemCpyDependence`

**Lines:** 1102-1265. Metadata copy at line 1256.

## Bug

The memcpy-of-memcpy forwarding transform creates a new outer memcpy
(`NewM`) that copies from the original source through the intermediate
buffer. The only metadata copied is `MD_DIAssignID`:

```cpp
// line 1256
NewM->copyMetadata(*M, LLVMContext::MD_DIAssignID);
```

The outer memcpy `M` may carry `!alias.scope`, `!noalias`, `!tbaa`,
`!tbaa.struct`, or `!nontemporal` metadata describing the destination
write. All of it is dropped. There is also no `combineAAMetadata(NewM, MDep)`
to merge in the dependency memcpy's AAMD. Compare with `processByValArgument`
(line 2070) and `processImmutArgument` (line 2174), both of which do call
`combineAAMetadata(&CB, MDep)` when forwarding through a memcpy.

Loss of `!alias.scope`/`!noalias` is a soundness-relevant regression for
downstream AA-driven passes (LICM, DSE, GVN). Loss of `!nontemporal`
silently defeats the hardware-cache hint the user requested.

## Confirmed via opt

```ll
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr captures(none), ptr captures(none),
                                    i64, i1)

define void @test(ptr noalias %dst, ptr noalias %src) {
  %tmp = alloca [32 x i8], align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %tmp, ptr align 8 %src,
                                   i64 32, i1 false)
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %tmp,
                                   i64 32, i1 false),
       !alias.scope !0, !noalias !4, !nontemporal !7
  call void @use(ptr %dst)
  ret void
}

!0 = !{!1}
!1 = distinct !{!1, !2, !"scope"}
!2 = distinct !{!2, !"domain"}
!4 = !{!5}
!5 = distinct !{!5, !2, !"scope2"}
!7 = !{i32 1}
```

After `opt -passes=memcpyopt -S`:

```ll
define void @test(ptr noalias %dst, ptr noalias %src) {
  %tmp = alloca [32 x i8], align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %tmp, ptr align 8 %src,
                                   i64 32, i1 false)
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src,
                                   i64 32, i1 false)   ; ALL METADATA GONE
  call void @use(ptr %dst)
  ret void
}
```

The intermediate memcpy is intentionally kept because something else uses
`%tmp` here (it is also visible from the alloca through `@use`), but the
forwarding to %src happens. The dropped metadata is the bug regardless of
the surrounding DSE outcome.

## Fix sketch

```cpp
NewM->copyMetadata(*M, LLVMContext::MD_DIAssignID);
combineAAMetadata(NewM, M);
combineAAMetadata(NewM, MDep);
if (auto *NT = M->getMetadata(LLVMContext::MD_nontemporal))
  NewM->setMetadata(LLVMContext::MD_nontemporal, NT);
```

(There is no `!nontemporal` form for the source side because memcpy is the
join of two accesses; conservative thing is to honor it whenever set on the
outer memcpy.)
