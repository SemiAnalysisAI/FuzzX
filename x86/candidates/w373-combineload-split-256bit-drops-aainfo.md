# w373: X86 combineLoad split of 256-bit load drops AAMDNodes on both halves

## Summary

`combineLoad` splits a 256-bit vector load into two 128-bit loads when the
target is either non-AVX2 nontemporal-aligned or a slow-256-bit-load CPU. The
two new loads are created with the `getLoad(VT, dl, Chain, Ptr, PtrInfo,
Align, MMOFlags)` overload, which defaults `AAInfo` to empty. As a result,
`!alias.scope` / `!noalias` / `!tbaa` from the original load are dropped on
both halves.

This is the load-side mirror of w371 (`splitVectorStore`). It is independent
code in `combineLoad` (i.e. the bug exists in two different functions and both
should be fixed).

## Source

```
llvm/lib/Target/X86/X86ISelLowering.cpp:54116-54122
    SDValue Load1 =
        DAG.getLoad(HalfVT, dl, Ld->getChain(), Ptr1, Ld->getPointerInfo(),
                    Ld->getBaseAlign(), Ld->getMemOperand()->getFlags());
    SDValue Load2 =
        DAG.getLoad(HalfVT, dl, Ld->getChain(), Ptr2,
                    Ld->getPointerInfo().getWithOffset(HalfOffset),
                    Ld->getBaseAlign(), Ld->getMemOperand()->getFlags());
```

The companion bool-vector-load cast at lines 54137-54139 has the same shape
and same bug, as does the v8i1 LowerLoad path (w370).

## Reproducer

`/tmp/x86h/split_load.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <8 x i32> @l256(ptr %p) {
entry:
  %v = load <8 x i32>, ptr %p, align 1, !alias.scope !2, !noalias !5
  ret <8 x i32> %v
}

!1 = !{!"d"}
!2 = !{!3}
!3 = distinct !{!3, !1, !"a"}
!5 = !{!6}
!6 = distinct !{!6, !1, !"b"}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -mcpu=sandybridge
-stop-after=finalize-isel`:

Bug (both halves lose alias info):
```
    %1:vr128 = VMOVDQUrm %0, 1, $noreg, 0, $noreg :: (load (s128) from %ir.p, align 1)
    %4:vr256 = VINSERTF128rmi killed %2, %0, 1, $noreg, 16, $noreg, 1 :: (load (s128) from %ir.p + 16, align 1)
```

Control (`<4 x i32>` instead — no splitting):
```
    %1:vr128 = VMOVDQUrm %0, 1, $noreg, 0, $noreg :: (load (s128) from %ir.p, align 1, !alias.scope !0, !noalias !3)
```

## Impact

- Fires on common AVX targets with slow 256-bit loads (Sandy Bridge-class)
  and on nontemporal 256-bit loads without AVX2.
- Each missed `!alias.scope` blocks downstream MachineSink, LICM, scheduling
  reorderings that should otherwise be legal.

## Severity

Medium. Same scope as w371 on the load side. Mechanical fix: pass
`Ld->getAAInfo()` (and `Ld->getRanges()`) to both `getLoad` calls.
