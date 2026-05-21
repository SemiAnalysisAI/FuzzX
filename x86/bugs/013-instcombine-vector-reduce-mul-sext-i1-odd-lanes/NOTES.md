# 013 — InstCombine `vector_reduce_mul(sext(<n x i1>))` drops sign for odd lane counts

Component: Transforms/InstCombine/InstCombineCalls.cpp

## Source

Around `llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:4112-4137`,
InstCombine matches `vector_reduce_mul(?ext(<n x i1>))` and rewrites it to
`zext(and-reduce(V))` — i.e. "the product of an all-{0,1} vector is non-zero
iff every lane is 1, so it's the AND of the lanes, zero-extended back."

That reasoning is sound for `zext(<n x i1>)` (lanes 0/1, product is 0 or 1).
It is **wrong for `sext(<n x i1>)`** (lanes 0/-1) and / or odd `n`:
- `sext` produces lanes `-1`, so the product is `(-1)^popcount(true)`.
- With odd `n` and all-true input, the product is `-1`, not `+1`.

The fold uses `CreateZExt` unconditionally and ignores the sign of the
original `?ext` and the lane-count parity.

## Runtime demonstration

`repro.ll`:

```ll
define i8 @f(<3 x i1> %m) {
  %s = sext <3 x i1> %m to <3 x i8>
  %r = call i8 @llvm.vector.reduce.mul.v3i8(<3 x i8> %s)
  ret i8 %r
}
```

After `opt -passes=instcombine -S`:

```ll
define i8 @f(<3 x i1> %m) {
  %1 = bitcast <3 x i1> %m to i3
  %2 = icmp eq i3 %1, -1
  %r = zext i1 %2 to i8        ; <-- returns 0 or 1, never -1
  ret i8 %r
}
```

Compare against InstSimplify on the all-constant input — it correctly
yields `-1`:

```ll
define i8 @all_true() {
  ret i8 -1              ; correct
}
```

`runner.c` calls `f(1, 1, 1)`. Output:

```
vector_reduce_mul(sext(<i1 1,1,1>)) = 1 (expected -1)
FAIL — odd-lane parity dropped (got +1 instead of -1)
```

## Why this is a wrong-code bug

`@llvm.vector.reduce.mul.v3i8(<-1,-1,-1>)` is mathematically
`(-1) * (-1) * (-1) = -1`. The InstCombine rewrite produces `+1`. Any
caller doing arithmetic with the result downstream gets a sign-flipped
value — a real miscompile.

For even `n`, the parity happens to land on `+1` and the fold is correct
(though only by accident — `zext` is still wrong for non-all-true inputs
with mixed lanes when sext was used). For odd `n` (n=1, 3, 5, 7, 9, ...),
the all-true case is provably wrong.

## Fix sketch

Either:
1. Bail out of the fold when the source is `sext` (only allow `zext`).
2. Or: if `sext`, emit `select(and-reduce(V), -1, 0)` (i.e. sext the i1
   result), and additionally guard on even `n` for safety.

## Files
- `repro.ll`  — IR
- `runner.c`  — calls `f(1,1,1)`; expects -1, observes +1
- `cmd.sh`    — dumps IR after InstCombine + the bad asm + runs the binary
