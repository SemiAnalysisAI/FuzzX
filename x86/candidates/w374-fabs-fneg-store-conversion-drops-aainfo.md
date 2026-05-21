# w374: X86 scalar fabs/fneg load-store-to-int conversion drops AAMDNodes on store

## Summary

The DAG combine in `combineStore` that converts a scalar `f16`/`bf16`/`f32`/
`f64` `fneg`/`fabs(load)`-and-store pattern into integer XOR/AND-and-store
issues the new store via `getStore(Chain, dl, Val, Ptr, PtrInfo, Align,
MMOFlags)`. That overload defaults `AAInfo` to empty, so `!alias.scope`,
`!noalias`, `!tbaa` on the original store are silently dropped.

Notably, in the same combine, the **load** stays alive (it's only bitcast),
so its AAInfo is preserved. Only the *store* loses metadata, which is easy to
miss in a code review.

## Source

```
llvm/lib/Target/X86/X86ISelLowering.cpp:54761-54782
  // Convert scalar fabs/fneg load-store to integer equivalents.
  if ((VT == MVT::f16 || VT == MVT::bf16 || VT == MVT::f32 || VT == MVT::f64) &&
      (StoredVal.getOpcode() == ISD::FABS ||
       StoredVal.getOpcode() == ISD::FNEG) &&
      ISD::isNormalLoad(StoredVal.getOperand(0).getNode()) &&
      StoredVal.hasOneUse() && StoredVal.getOperand(0).hasOneUse()) {
    MVT IntVT = VT.getSimpleVT().changeTypeToInteger();
    if (TLI.isTypeLegal(IntVT)) {
      APInt SignMask = APInt::getSignMask(VT.getScalarSizeInBits());
      unsigned SignOp = ISD::XOR;
      if (StoredVal.getOpcode() == ISD::FABS) {
        SignMask = ~SignMask;
        SignOp = ISD::AND;
      }
      SDValue LogicOp = DAG.getNode(
          SignOp, dl, IntVT, DAG.getBitcast(IntVT, StoredVal.getOperand(0)),
          DAG.getConstant(SignMask, dl, IntVT));
      return DAG.getStore(St->getChain(), dl, LogicOp, St->getBasePtr(),
                          St->getPointerInfo(), St->getBaseAlign(),
                          St->getMemOperand()->getFlags());
    }
  }
```

## Reproducer

`/tmp/x86h/fneg_st.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @fneg_st(ptr %p, ptr %q) {
entry:
  %v = load float, ptr %p, align 4, !alias.scope !2, !noalias !5
  %n = fneg float %v
  store float %n, ptr %q, align 4, !alias.scope !5, !noalias !2
  ret void
}

!1 = !{!"d"}
!2 = !{!3}
!3 = distinct !{!3, !1, !"a"}
!5 = !{!6}
!6 = distinct !{!6, !1, !"b"}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel`:

```
    %2:gr32 = MOV32ri -2147483648
    %3:gr32 = XOR32rm %2, %0, 1, $noreg, 0, $noreg, implicit-def dead $eflags :: (load (s32) from %ir.p, !alias.scope !0, !noalias !3)
    MOV32mr %1, 1, $noreg, 0, $noreg, killed %3 :: (store (s32) into %ir.q)
```

Bug: the load keeps `!alias.scope !0, !noalias !3`, but the store has no alias
metadata at all. It should be `(store (s32) into %ir.q, !alias.scope !3,
!noalias !0)`.

Control: remove the `fneg`, write `%v` straight through. Then the store keeps
its metadata.

## Impact

- Fires on the common scalar `fneg`/`fabs` of a load followed by a store of
  the result — a pattern that appears whenever C code negates a `float`/
  `double` it just loaded from memory.
- Silently disables MachineSink / LICM / scheduler reorderings that rely on
  the dropped store-side AA metadata.

## Severity

Low-medium. No miscompile but a sneaky, easy-to-miss leak in a hot scalar FP
combine. Fix: pass `St->getAAInfo()` (and `St->getRanges()`) to the
`getStore` call.

This is the same family as w370/w371/w372/w373: in `X86ISelLowering.cpp`, the
`SelectionDAG::getStore(...)` / `getLoad(...)` overload that drops `AAInfo`
is consistently chosen across multiple combines. A systematic audit and
switch to the `MachineMemOperand*`-taking overloads (so all MMO metadata is
preserved by construction) would prevent the pattern.
