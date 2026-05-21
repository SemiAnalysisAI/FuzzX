# w244: (X & (X ashr 31)) folded to smin(X, 0)

## File / Region
- `llvm/lib/Transforms/InstCombine/InstCombineAndOrXor.cpp` (visitAnd
  area). Likely an idiom recognition in `foldAndOfICmps` or
  `visitAnd`'s pattern matching.

## Code
The fold recognizes `X & (X >>s (bitwidth-1))` as the sign-mask idiom.

## Observation
`X & (X ashr 31)` is the "non-negative ? 0 : X" idiom. The fold
canonicalizes it to `smin(X, 0)`.

## Analysis (Alive2-style)
For X >= 0:
- `X ashr 31` = 0 (sign bit is 0, replicated as 0...0).
- `X & 0` = 0.
- `smin(X, 0)` = 0 (since X >= 0).
- **Match.**

For X < 0:
- `X ashr 31` = -1 (sign bit is 1, replicated as 1...1 = -1).
- `X & -1` = X.
- `smin(X, 0)` = X (since X < 0).
- **Match.**

The fold is **correct.**

## Reproducer
Source: `/tmp/w240/t31_select_simp.ll`

```llvm
define i32 @and_shr(i32 %x) {
  %s = ashr i32 %x, 31
  %r = and i32 %x, %s
  ret i32 %r
}
```

`opt -passes=instcombine -S` output:
```llvm
define i32 @and_shr(i32 %x) {
  %r = call i32 @llvm.smin.i32(i32 %x, i32 0)
  ret i32 %r
}
```

## Verdict
**NOT a miscompile.** Canonicalization to `smin` intrinsic — correct
behavior. Documented for completeness.
