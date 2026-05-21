# matchAddressRecursively MUL case: signed-by-unsigned product feeds foldOffsetIntoAddress

**File**: `llvm/lib/Target/X86/X86ISelDAGToDAG.cpp:2670-2685`

## Reasoning

In the `X*[3,5,9]` -> `X+X*[2,4,8]` rewrite, the constant addend in
`(MulVal = ADD x, c) * {3,5,9}` is computed as:

```cpp
auto *AddVal = cast<ConstantSDNode>(MulVal.getOperand(1));
uint64_t Disp = AddVal->getSExtValue() * CN->getZExtValue();
if (foldOffsetIntoAddress(Disp, AM))
  Reg = N.getOperand(0);
```

`AddVal->getSExtValue()` returns `int64_t`; `CN->getZExtValue()` is
`uint64_t`. The multiplication is performed in `uint64_t` (the signed
operand is sign-extended to int64, then implicitly converted to uint64
for the operator). When `AddVal` is negative (e.g. -1) and CN is 5, the
mathematical product `-5` becomes `0xFFFFFFFFFFFFFFFB` as a `uint64_t`,
and `Disp` is then passed to `foldOffsetIntoAddress(uint64_t Offset,
…)`. Inside `foldOffsetIntoAddress`:

```cpp
int64_t Val = AM.Disp + Offset;
```

`AM.Disp` is `int32_t` so the sum runs in `uint64_t`, then assigned to
`int64_t`, then `isOffsetSuitableForCodeModel(Val, M, …)` evaluates the
result as signed. For `AM.Disp = 0` and Offset = -5 (as uint64_t huge),
`Val` becomes -5 — correct. For `AM.Disp = 1000` and Offset = -5,
`Val = 995` — correct. So the arithmetic is fine *if both sign-extended
to 64 bits agree on signedness*. The issue arises when `CN->getZExtValue()
>= 2` and `AddVal` has bits in the upper 32 — the product overflows in
uint64. Since x86-64 globals only use a 32-bit disp this generally
catches via `isOffsetSuitableForCodeModel` returning false.

There's a corner: when `CN == 9` and `AddVal == 0x2000_0000`
(positive 32-bit), product = 0x12_0000_0000 which fits in int64. Then
`isOffsetSuitableForCodeModel` rejects (exceeds 32-bit). Backoff path
sets `Reg = N.getOperand(0)` which is the *original mul operand*, not
the ADD operand X — so AM.Scale stays 9-1=8, AM.IndexReg = AM.Base_Reg
= MulVal (the full ADD). The intent of the rewrite was lost; we now
emit `lea (ADD,ADD,8)` which **doubles the value of the ADD inside the
addressing mode**, equivalent to `9 * (X + c)` — but the SDNode that's
selected actually computes `9 * MulVal = 9 * (X + c)`, while the
matcher selected `ADD + ADD * 8 = 9 * MulVal`. That part is consistent.
So this is benign. **Updating: not a bug.**

The remaining hazard: when `foldOffsetIntoAddress` succeeds, `Reg` is
left uninitialized in the `else` branch of the MulVal-add check (line
2673-2680). Reading the code again:

```cpp
if (MulVal.getNode()->getOpcode() == ISD::ADD && MulVal.hasOneUse() &&
    isa<ConstantSDNode>(MulVal.getOperand(1))) {
  Reg = MulVal.getOperand(0);
  ...
  if (foldOffsetIntoAddress(Disp, AM))
    Reg = N.getOperand(0);
} else {
  Reg = N.getOperand(0);
}
AM.IndexReg = AM.Base_Reg = Reg;
```

This is OK — Reg is always assigned. So the only real risk is the
multiplication overflow producing a Disp that *looks* in-range (e.g.
fits in 31 bits after wraparound). For `AddVal == 0x40000001`, CN == 3,
the int64 product is 0xC0000003. `foldOffsetIntoAddress` calls
`isOffsetSuitableForCodeModel(0xC0000003, Small, hasSymbolic=false)`
which checks isInt<31>. 0xC0000003 doesn't fit signed 32, so it
rejects. Good.

For `AddVal == -0x10000000`, CN == 3, product is `-0x30000000` =
`0xFFFFFFFFD0000000`. As int64 that's -805306368 which fits int31.
Result `Val = AM.Disp + Offset = 0 + (-0x30000000) = -0x30000000`
which is valid. OK.

The truly worrying case: `AddVal` is a *32-bit* constant whose
`getSExtValue()` doesn't match how the matcher treats it. ConstantSDNode
of value 0xFFFFFFFF in a `MulVal.getValueType() == MVT::i32` context
sign-extends to -1, so `AddVal->getSExtValue() == -1`. Product with
CN=3 is -3 (uint64 huge). Disp folded as -3 — but the original DAG had
`(X + 0xFFFFFFFF) * 3` which in i32 wraps; the lowered LEA computes
`X + (X * (3-1)) + (-3)` = `3X - 3`. The original wrap is `(X + (-1))
* 3 = 3X - 3`. **Match.** OK, this is fine in i32 because the
multiplier and add semantics are both wrap-mod-2^32.

Filing this as a low-confidence candidate to investigate fuzzer hits
around `lea`-folded MUL-by-{3,5,9} with negative or 32-bit-wraparound
constants in 64-bit pointer math, particularly when `MulVal`'s VT is
i32 and the LEA selected is `LEA64`.

## Repro sketch

```ll
define ptr @f(i32 %x) {
  %a = add nsw i32 %x, -1
  %b = mul nsw i32 %a, 5
  %c = sext i32 %b to i64
  %p = getelementptr i8, ptr null, i64 %c
  ret ptr %p
}
```

Inspect the LEA selected.

## Wrong outcome

If the matcher accepts a folded `Disp` from a sign/zero-extend mismatch,
the addressing mode yields a different effective pointer than the DAG
node computed (off by ~`(CN-1) * 2^32`). On 64-bit targets the extra
range may still produce a "valid" pointer that loads garbage.

## Cross-reference

`llvm/test/CodeGen/X86/lea-3.ll`, `lea-opt.ll`, `lea-add.ll` cover the
common cases but I did not find one mixing a *negative* `AddVal` with
the `*{3,5,9}` shortcut.
