# InstructionSimplify folds vp.reverse(vp.reverse(X)) -> X without checking outer mask

## Summary
`llvm/lib/Analysis/InstructionSimplify.cpp:7448-7451` folds
`vp.reverse(vp.reverse(X, mask1, EVL), mask2, EVL) -> X` matching only the
inner mask (`m_Value()`) and the EVL. The outer mask `mask2` is dropped.

LangRef defines `llvm.experimental.vp.reverse`:
```
The lanes in the result vector disabled by mask are poison.
```
So `vp.reverse(any, mask=all-false, EVL=any) = <poison, poison, ...>`,
not the input. The double-reverse fold treats two reverses as identity
regardless of the second mask, miscompiling any lane disabled in the outer
mask from "poison" to a defined value.

The companion fold for splat at 7454-7455 is fine
(`vp.reverse(splat(X)) -> splat(X)`) because lanes that are *not* covered by
the result still appear under the splat description "splat(X) where masked
lanes are poison": actually the comment claims "regardless of mask and EVL"
but the same poison-on-disabled-mask rule applies, so this companion fold is
*also* wrong by the same reasoning when `mask` has 0 lanes and `X` is a
defined non-poison value (splat(X) with mask=0 is poison-on-every-lane, not
splat(X)). Both folds inherit the same blind-spot.

## Reproducer
```llvm
define <4 x i32> @t1(<4 x i32> %x, i32 %evl) {
  %a = call <4 x i32> @llvm.experimental.vp.reverse.v4i32(
        <4 x i32> %x,
        <4 x i1>  <i1 1, i1 1, i1 1, i1 1>,
        i32 %evl)
  %b = call <4 x i32> @llvm.experimental.vp.reverse.v4i32(
        <4 x i32> %a,
        <4 x i1>  <i1 0, i1 0, i1 0, i1 0>,   ; outer mask: ALL OFF
        i32 %evl)
  ret <4 x i32> %b
}
declare <4 x i32> @llvm.experimental.vp.reverse.v4i32(<4 x i32>, <4 x i1>, i32)
```

`opt -O2 -S`:
```llvm
define <4 x i32> @t1(<4 x i32> returned %x, i32 %evl) ... {
  ret <4 x i32> %x
}
```

Expected: every lane of the result is poison (or at minimum, the IR retains
the outer `vp.reverse` so the lowerer applies the mask). Got: the caller is
told `ret <4 x i32> %x`, i.e. returns the original input unchanged. A
downstream `freeze` on the result, or any branch on a lane that the user
relied on being poison, is now miscompiled.

## Root cause
`llvm/lib/Analysis/InstructionSimplify.cpp:7443-7457`:
```c++
case Intrinsic::experimental_vp_reverse: {
  Value *Vec = Call->getArgOperand(0);
  Value *EVL = Call->getArgOperand(2);

  Value *X;
  // vp.reverse(vp.reverse(X)) == X (mask doesn't matter)
  if (match(Vec, m_Intrinsic<Intrinsic::experimental_vp_reverse>(
                     m_Value(X), m_Value(), m_Specific(EVL))))
    return X;                                  // <-- ignores OUTER call's mask

  // vp.reverse(splat(X)) -> splat(X) (regardless of mask and EVL)
  if (isSplatValue(Vec))
    return Vec;                                // <-- same blind spot
  return nullptr;
}
```

`Call->getArgOperand(1)` (the outer mask) is never read. The
`m_Value()` slot inside `m_Intrinsic` matches the *inner* mask; that is
fine — the inner mask only affects how the inner reverse populated the
intermediate vector, which the outer reverse then permutes. The problem is
the *outer* mask: any lane it disables must be poison in the result, but the
fold short-circuits to `X` without leaving a `freeze`/`vp.reverse` to enforce
that.

## Fix sketch
Gate both folds on the outer mask being known all-ones:
```c++
Value *OuterMask = Call->getArgOperand(1);
if (match(OuterMask, m_AllOnes())) {
  // existing folds OK
}
```
(or insert a `vp.merge`/`select` with poison for disabled lanes, but
the `m_AllOnes` gate is simpler and matches the comment's intent).

## Notes
- EVL handling is correct (both calls must agree, `m_Specific(EVL)`).
- This was added by the same patch that established the splat fold; both
  comments admit "mask doesn't matter" / "regardless of mask and EVL" — both
  claims contradict the LangRef.
- I did not find a prior candidate addressing the vp.reverse double-fold
  mask-ignoring issue (closest is w262/w515-style FMF folds, distinct).
