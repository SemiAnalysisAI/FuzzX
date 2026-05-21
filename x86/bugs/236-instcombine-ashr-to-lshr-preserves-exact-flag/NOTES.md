# 236 — InstCombine SimplifyDemandedUseBits `ashr exact` → `lshr exact` preserves `exact` flag — anti-refinement

Component: `llvm/lib/Transforms/InstCombine/InstCombineSimplifyDemanded.cpp` lines ~925-931

The fold has two arms. The `Known.Zero[BitWidth-1]` arm (input non-negative) is safe — `ashr exact` and `lshr exact` coincide. The `!ShiftedInBitsDemanded` arm is value-correct in demanded bits, but `setIsExact(true)` is wrong: `ashr exact` requires shifted-off bits to equal the **sign bit**, while `lshr exact` requires them to equal **0**. For negative inputs with low-bits set, the source is defined but the rewrite is poison.

## Reproducer

```ll
%a = ashr exact i32 %x, 3
%m = and i32 %a, 255
```

`opt -passes=instcombine -S` → `%a = lshr exact i32 %x, 3`.

For `%x = -1`: source returns `255` (defined). Optimized returns `poison` (since `lshr exact` of `-1` by 3 shifts off three 1-bits, which is not 0).

## Severity

Default x86 -O2. Anti-refinement — turns a defined value into poison. Alive2-falsifiable.

The existing test `ashr_can_be_lshr` in `llvm/test/Transforms/InstCombine/ashr-demand.ll` asserts the buggy output. The comment "if it does not matter if we do ashr or lshr" is incorrect — it matters when `exact` is set.

## Fix

In the `!ShiftedInBitsDemanded` arm, do NOT set `exact` on the new `lshr` — only the `Known.Zero[BitWidth-1]` arm justifies preserving it.
