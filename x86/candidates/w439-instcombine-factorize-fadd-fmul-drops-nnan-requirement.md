# InstCombine `factorizeFAddFSub` `(X*Z)+(Y*Z) -> (X+Y)*Z` requires only reassoc+nsz — turns NaN into a finite value

File: `llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp`, function
`factorizeFAddFSub`, lines 2029-2072. Called from `visitFAdd` (line 2136)
and `visitFSub` (line 3384) inside `if (I.hasAllowReassoc() &&
I.hasNoSignedZeros())` blocks.

## Pattern

```cpp
static Instruction *factorizeFAddFSub(BinaryOperator &I,
                                      InstCombiner::BuilderTy &Builder) {
  assert((I.getOpcode() == Instruction::FAdd ||
          I.getOpcode() == Instruction::FSub) && "Expecting fadd/fsub");
  assert(I.hasAllowReassoc() && I.hasNoSignedZeros() &&
         "FP factorization requires FMF");

  // ...
  // (X * Z) + (Y * Z) --> (X + Y) * Z
  // (X * Z) - (Y * Z) --> (X - Y) * Z
  // (X / Z) + (Y / Z) --> (X + Y) / Z
  // (X / Z) - (Y / Z) --> (X - Y) / Z
  // ...
}
```

The asserts confirm only `reassoc + nsz` are required. The transform changes
`a*z ± b*z` to `(a±b)*z`. When `a*z` and `b*z` individually overflow to
opposite-sign infinities while `a±b` is finite (or zero), the source
produces NaN (`inf − inf` / `inf + −inf`) but the fold produces a finite
value.

## Repro

```llvm
; opt -passes=instcombine -S
define float @bug(float %x, float %y, float %z) {
  %a = fmul reassoc nsz float %x, %z
  %b = fmul reassoc nsz float %y, %z
  %r = fadd reassoc nsz float %a, %b
  ret float %r
}
```

Output:

```llvm
define float @bug(float %x, float %y, float %z) {
  %1 = fadd reassoc nsz float %x, %y
  %r = fmul reassoc nsz float %1, %z
  ret float %r
}
```

Witness (X = 1e30, Y = -1e30, Z = 1e30 — Z chosen so each fmul overflows
in f32):

```llvm
define float @check() {
  %a = fmul reassoc nsz float 1.0e30, 1.0e30   ; +inf
  %b = fmul reassoc nsz float -1.0e30, 1.0e30  ; -inf
  %r = fadd reassoc nsz float %a, %b           ; +inf + -inf = +qnan
  ret float %r
}
; folds to: ret float +qnan
```

So with `%x = 1e30, %y = -1e30, %z = 1e30`:
- Source returns `+qnan`.
- Folded function returns `(1e30 + -1e30) * 1e30 = 0 * 1e30 = 0`.

## Why this is wrong

`nsz` only authorizes ignoring the sign of zeros; `reassoc` authorizes
reassociation but not the elimination of NaN results. Turning `inf - inf
= NaN` into a non-NaN value requires `nnan` (the user attesting that the
source never reaches the inf-inf case).

The same defect applies to the three sibling rewrites listed in the
comment block:

- `(X * Z) - (Y * Z) -> (X - Y) * Z` — same overflow + opposite-sign
  scenario (X = 1e30, Y = 1e30 + something distinguishable, Z = 1e30 such
  that `X*Z` and `Y*Z` both overflow to `+inf` and the subtraction
  underflows to a representable value vs. NaN).
- `(X / Z) + (Y / Z) -> (X + Y) / Z` — for `Z` very small and `X, Y`
  large the source's `X/Z + Y/Z` can overflow component-wise while
  `(X+Y)/Z` does not, or vice versa; and for `Z = 0` (with `arcp`/no
  `ninf`) the source can produce `inf − inf = NaN` while `(X+Y)/0` is
  signed inf.
- `(X / Z) - (Y / Z) -> (X - Y) / Z` — same.

## Suggested fix

Tighten the gating that calls `factorizeFAddFSub`:

```cpp
if (I.hasAllowReassoc() && I.hasNoSignedZeros() && I.hasNoNaNs() &&
    I.hasNoInfs()) {
  if (Instruction *F = factorizeFAddFSub(I, Builder)) ...
}
```

and update the asserts inside `factorizeFAddFSub` accordingly. `nnan`
prevents the inf±inf NaN, and `ninf` prevents the case where one form
overflows and the other does not. Less invasive alternative: gate only the
fmul-factoring variants (where overflow can flip finite to inf) on `ninf`,
since the fdiv variants have different failure modes.
