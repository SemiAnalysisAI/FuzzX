# w372: X86 ptr32/ptr64 addrspace cast in combineLoad drops AAMDNodes

## Summary

In `combineLoad`, when the load uses one of the size-mismatched X86 address
spaces (`X86AS::PTR64`, `PTR32_SPTR`, `PTR32_UPTR`) and the pointer needs to be
cast to the default pointer type, the lowering rebuilds the load via
`DAG.getExtLoad(Ext, dl, RegVT, Chain, Cast, PtrInfo, MemVT, Align, MMOFlags)`.
This overload defaults `AAInfo` to `AAMDNodes()`, so `Ld->getAAInfo()` is lost
on the new load. `!alias.scope` / `!noalias` / `!tbaa` metadata are silently
stripped during this lowering.

## Source

```
llvm/lib/Target/X86/X86ISelLowering.cpp:54172-54184
  // Cast ptr32 and ptr64 pointers to the default address space before a load.
  unsigned AddrSpace = Ld->getAddressSpace();
  if (AddrSpace == X86AS::PTR64 || AddrSpace == X86AS::PTR32_SPTR ||
      AddrSpace == X86AS::PTR32_UPTR) {
    MVT PtrVT = TLI.getPointerTy(DAG.getDataLayout());
    if (PtrVT != Ld->getBasePtr().getSimpleValueType()) {
      SDValue Cast =
          DAG.getAddrSpaceCast(dl, PtrVT, Ld->getBasePtr(), AddrSpace, 0);
      return DAG.getExtLoad(Ext, dl, RegVT, Ld->getChain(), Cast,
                            Ld->getPointerInfo(), MemVT, Ld->getBaseAlign(),
                            Ld->getMemOperand()->getFlags());
    }
  }
```

`SelectionDAG::getExtLoad(...PtrInfo, MemVT, Align, MMOFlags, AAInfo)` —
`AAInfo` defaults to empty.

## Reproducer

`/tmp/x86h/ptr32_load.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-p:64:64-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"

define i32 @loadp32(ptr addrspace(270) %p) {
entry:
  %v = load i32, ptr addrspace(270) %p, align 1, !alias.scope !2, !noalias !5
  ret i32 %v
}

!1 = !{!"d"}
!2 = !{!3}
!3 = distinct !{!3, !1, !"a"}
!5 = !{!6}
!6 = distinct !{!6, !1, !"b"}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel`:

Bug (alias.scope/noalias missing):
```
    %2:gr64 = MOVSX64rr32 killed %1
    %3:gr32 = MOV32rm killed %2, 1, $noreg, 0, $noreg :: (load (s32) from %ir.p, align 1, addrspace 270)
```

Control (addrspace 272 / PTR64 — sign-extension is a no-op so the path doesn't
trigger):
```
    %1:gr32 = MOV32rm %0, 1, $noreg, 0, $noreg :: (load (s32) from %ir.p, align 1, !alias.scope !0, !noalias !3, addrspace 272)
```

The bug only fires when the size-mismatched cast path is actually entered, i.e.
PTR32_SPTR/PTR32_UPTR on 64-bit target, or PTR64 on 32-bit target.

## Impact

Affects MS-ABI / mixed-mode code (e.g. WoW64 ptr32 in 64-bit code) using TBAA
or noalias annotations. Lost alias metadata silently disables MachineSink /
LICM / scheduler reorderings across these loads. There is no store-side
equivalent of this specific block in `combineStore` (the address-space cast
gate is on the load side only), so this candidate is load-only.

## Severity

Low-medium. Targets a niche subset (X86AS::PTR32_SPTR/UPTR with mismatched
PtrVT) but is real and affects real Windows-ABI codegen.
