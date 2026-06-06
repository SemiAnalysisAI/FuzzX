# 042 - `fshl(0, X, C)` creates lane poison for zero shift amounts

Component: `llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:2577`

`llvm.fshl` treats the shift amount as unsigned modulo the element width. For
an 8-bit lane, shift amount `0` means `0`, not `8`. InstCombine first
canonicalizes constant shift amounts modulo the bitwidth, then rewrites:

```llvm
fshl(0, X, C) -> lshr X, (BW - C)
```

That rewrite is not valid when a lane of `C` is zero, because `BW - 0 == BW`
and an LLVM `lshr` by the bitwidth is poison.

The reproducer folds:

```llvm
%r = call <2 x i8> @llvm.fshl.v2i8(
  <2 x i8> zeroinitializer,
  <2 x i8> %x,
  <2 x i8> <i8 0, i8 1>)
```

to:

```llvm
%r = lshr <2 x i8> %x, <i8 8, i8 7>
```

Witness: source lane 0 is `fshl(0, x0, 0)`, which extracts the high 8 bits of
`concat(0, x0) << 0`, so it is always `0`. The optimized lane 0 is
`lshr i8 x0, 8`, which is poison. The `freeze` in the reproducer can expose a
nonzero result from the optimized program, while the source always returns
zero.

This is an ordinary integer/vector InstCombine miscompile. It has no pointer
representation issue, fast-math flags, or strictfp interaction.

Verifier: Turing the 2nd (019e9987-5241-7c01-b621-69296ed46486) returned YES.
