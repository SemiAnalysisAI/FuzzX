# 253 — InstCombine `foldAddLikeCommutative` over-infers `nsw` on rewritten add from `or disjoint`

Component: `llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp` lines ~1355-1370 (`foldAddLikeCommutative`), invoked from `InstCombineAndOrXor.cpp:4172-4180` in `visitOr`.

The fold pattern `or disjoint (add nsw A, C1), (B & ~C1)` → `add nsw A, (or B, C1)` carries `nsw` forward from the inner add. But `or disjoint` only guarantees the *unsigned* sum fits (no carry); it does NOT guarantee the *signed* sum fits.

## Reproducer

```ll
define i8 @bug_or_disjoint(i8 %a, i8 %b_in) {
  %lhs = add nsw i8 %a, 5
  %rhs = and i8 %b_in, 250         ; ~5 in i8
  %r   = or disjoint i8 %lhs, %rhs
  ret i8 %r
}
```

`opt -passes=instcombine -S`:
```
%1 = or i8 %b_in, 5
%r = add nsw i8 %a, %1
```

For `%a = 100, %b_in = 130`:
- Source: `%lhs = 105` (no overflow), `%rhs = 130`, `or disjoint 105, 130 = 235 = -21 i8` (concrete).
- Optimized: `or 130, 5 = 135`, `add nsw 100, 135 = 235` — exceeds INT8_MAX (127), signed-overflow → **poison**.

Real Alive2-falsifiable miscompile. Optimizer introduced poison where the source returned a defined value.

## Severity

Default x86 -O2. The fold is common (mix of `add nsw` and `or disjoint` is a typical bit-packing pattern).

## Fix

Drop `nsw` on the new add unless either:
- the source `or` was `or disjoint`-with-positive-mask AND A is provably non-negative, OR
- the bit-positions are such that signed-overflow is provably impossible.

Conservative fix: just drop `nsw` (and rely on subsequent passes to re-derive if provable).

---

## CORRECTION (re-audit at HEAD `023e7decf625`) — NOT A BUG

The "Reproducer" arithmetic above is **wrong**. It claims `add nsw 100, 135`
overflows because `235 > INT8_MAX`, but the second operand `135` does not fit
in i8: as an i8 it is `0x87 = −121`, and `100 + (−121) = −21` is in `[−128,127]`
— no signed overflow, no poison. The optimized value equals the source value
(`235` unsigned = `−21` signed).

Brute force over **all** `i8 × i8` inputs for `C1 ∈ {1, 5, 64, 127, 250}`
(transform `or disjoint (add nsw a,C1),(b&~C1)` → `add nsw a,(b|C1)`) finds
**zero** inputs where the source is defined but the optimized form is poison
or a different value. The `nsw` (and `nuw`) forwarding is sound:

- `or disjoint X, Y` means `X & Y == 0`, so `X + Y == X | Y` with no carry.
- When `add nsw a, C1` does not overflow and the disjointness holds, the
  reassociated `add nsw a, (b|C1)` is provably non-overflowing as well.

The `foldAddLikeCommutative(..., /*NSW=*/true, /*NUW=*/true)` call from the
disjoint-`or` site in `visitOr` is therefore correct. **WONTFIX.**
