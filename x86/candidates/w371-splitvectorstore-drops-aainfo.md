# w371: X86 splitVectorStore / scalarizeVectorStore drop AAMDNodes (alias.scope/noalias)

## Summary

`splitVectorStore` and `scalarizeVectorStore` rebuild a 256/512-bit or
nontemporal vector store as multiple narrower stores via the
`getStore(Chain, dl, Val, Ptr, PtrInfo, Align, Flags)` overload. That overload
takes no `AAMDNodes` and no `Ranges` — they default to empty — so the new
`MachineMemOperand`s lose any `!alias.scope`, `!noalias`, `!tbaa` etc. from the
source IR store.

This is more impactful than the vXi1 case (w370) because `splitVectorStore`
fires on common AVX targets: it is the standard path for under-aligned 256-bit
non-temporal stores (`LowerStore` line 54798-54807) and for slow 256-bit
stores (`LowerStore` 54784-54796), and `scalarizeVectorStore` handles MOVNTI
expansion for 128-bit non-temporal stores on non-SSE4A AVX (`LowerStore`
54810-54817).

The MMO `Flags` (e.g. `MONonTemporal`) ARE preserved because the code forwards
`Store->getMemOperand()->getFlags()`. Only the AA metadata is lost.

## Source

```
llvm/lib/Target/X86/X86ISelLowering.cpp:26289-26295  (splitVectorStore)
    SDValue Ch0 =
        DAG.getStore(Store->getChain(), DL, Value0, Ptr0, Store->getPointerInfo(),
                     Store->getBaseAlign(), Store->getMemOperand()->getFlags());
    SDValue Ch1 =
        DAG.getStore(Store->getChain(), DL, Value1, Ptr1,
                     Store->getPointerInfo().getWithOffset(HalfOffset),
                     Store->getBaseAlign(), Store->getMemOperand()->getFlags());

llvm/lib/Target/X86/X86ISelLowering.cpp:26326-26329  (scalarizeVectorStore)
    SDValue Ch =
        DAG.getStore(Store->getChain(), DL, Scl, Ptr,
                     Store->getPointerInfo().getWithOffset(Offset),
                     Store->getBaseAlign(), Store->getMemOperand()->getFlags());
```

Compare `SelectionDAG::getStore(Chain, dl, Val, Ptr, PtrInfo, Align, MMOFlags,
AAInfo)` — `AAInfo` defaults to empty `AAMDNodes()`. The fix is to thread
`Store->getAAInfo()` (and possibly to use the `MachineMemOperand*` overload,
constructed from a properly-offset PtrInfo).

The matching `splitVectorLoad` (and other split/scalarize load helpers, e.g.
`memOpsSplitLoad`) should be audited the same way; this report focuses on
stores where confirmed.

## Reproducer

`/tmp/x86h/split_store.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @s256(ptr %p, <8 x i32> %v) {
entry:
  store <8 x i32> %v, ptr %p, align 1, !alias.scope !2, !noalias !5
  ret void
}

!1 = !{!"d"}
!2 = !{!3}
!3 = distinct !{!3, !1, !"a"}
!5 = !{!6}
!6 = distinct !{!6, !1, !"b"}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -mcpu=sandybridge -stop-after=finalize-isel`:

Bug (256-bit split into two 128-bit stores, alias info dropped):
```
    VEXTRACTF128mri %0, 1, $noreg, 16, $noreg, %1, 1 :: (store (s128) into %ir.p + 16, align 1)
    VMOVDQUmr %0, 1, $noreg, 0, $noreg, killed %2 :: (store (s128) into %ir.p, align 1)
```

Control (`<4 x i32>` instead, no split happens):
```
    VMOVDQUmr %0, 1, $noreg, 0, $noreg, %1 :: (store (s128) into %ir.p, align 1, !alias.scope !0, !noalias !3)
```

Both halves of the split store should carry the source's `!alias.scope !0,
!noalias !3` (a `getWithOffset`-style adjustment on AAInfo is not required —
AAMDNodes are not offset-sensitive in MIR's representation).

## Impact

- Affects all AVX/AVX2 platforms with slow 256-bit stores (older Sandy
  Bridge-class CPUs) and any AVX target that hits an under-aligned 256/512-bit
  non-temporal vector store.
- Lost AA metadata silently disables MachineSink / MachineLICM / scheduler
  reordering across the two halves and with surrounding loads.
- No miscompile, but a real performance metadata leak in a hot codegen path.

## Severity

Medium. Wider blast radius than w370 because it fires on common AVX `-mcpu`
configurations. Mechanical fix: pass `Store->getAAInfo()` and (optionally)
`Store->getRanges()` to each `DAG.getStore` call.
