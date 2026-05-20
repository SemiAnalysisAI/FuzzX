# m076: `umin(amdgcn.sffbh(x), Clamp)` folds to `sffbh(x)` when `x` could be `-1` at runtime

Found by reading `performMinMaxCombine` in `SIISelLowering.cpp` (around line
16175 in current HEAD).  The fold:

```cpp
// umin(sffbh(x), bitwidth) -> sffbh(x) if x is known to be not 0 or -1.
SDValue FfbhSrc;
uint64_t Clamp = 0;
if (Opc == ISD::UMIN &&
    sd_match(Op0,
             m_IntrinsicWOChain<Intrinsic::amdgcn_sffbh>(m_Value(FfbhSrc))) &&
    sd_match(Op1, m_ConstInt(Clamp))) {
  unsigned BitWidth = FfbhSrc.getValueType().getScalarSizeInBits();
  if (Clamp >= BitWidth) {
    KnownBits Known = DAG.computeKnownBits(FfbhSrc);
    if (Known.isNonZero() && !Known.isAllOnes())
      return Op0;
  }
}
```

`amdgcn.sffbh(x)` returns the bit position of the first significant bit, or
`-1` (the unsigned `UINT_MAX`) when `x` is `0` or `-1`.  The fold replaces
`umin(sffbh(x), Clamp)` with `sffbh(x)` whenever `x` is "not 0 or -1" --
guaranteeing the result is in `[0, BW-1]`, which is `< Clamp >= BW`.

The intended check is "`x` is provably **not** `-1`", but the code uses
`!Known.isAllOnes()`.  These are not the same:

* `Known.isAllOnes()` returns `true` only if every bit of `x` is **known**
  to be `1`.
* `!Known.isAllOnes()` therefore returns `true` for any `x` with at least
  one unknown bit -- including values that *could* be `-1` at runtime.

So the fold fires whenever `x` is provably non-zero but has any unknown bit,
even if `x = -1` is reachable.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m076-sffbh-umin-knownbits-check/reduced.ll
```

The reduced IR loads an i32 input, computes `x = v | 1` (provably non-zero
because bit 0 is set, high bits unknown), then evaluates
`umin(sffbh(x), 32)`.  For the input `0xFFFFFFFE`, `x = -1` and the correct
result is `32` (because `sffbh(-1) = UINT_MAX` and `umin(UINT_MAX, 32) =
32`).

Observed output (LLVM HEAD with the local PR patches, `gfx950`):

```text
input=0xFFFFFFFE
O0=0x00000020   ; correct: 32
O2=0xFFFFFFFF   ; wrong: sffbh(-1) returned directly without the umin
mismatch=true
```

`-O0` skips the SDAG combiner so the umin is evaluated and clamps the
`-1` to `32`.  `-O2` runs the combiner, the `!Known.isAllOnes()` check
passes (because the high bits of `v | 1` are unknown), and the fold
returns `sffbh(x) = -1`.

## Fix sketch

Replace the weak negative check with a proven-not-all-ones check.  Two
options:

1. Use `Known.getMaxValue() != Known.getMaxValue().getAllOnes()` (asks for
   the max possible value to be strictly less than `-1`).
2. Check that at least one bit is `Known.Zero` -- if any bit is provably
   `0`, then `x` cannot be all-ones:

   ```cpp
   if (Known.isNonZero() && Known.Zero.getBoolValue())
     return Op0;
   ```

The parallel `Known.isNonZero()` already uses the strict "provably ..."
form, so making the second test symmetric is the obvious fix.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Passes: `O0=O2=0x00000020`. The buggy fold must have changed shape between 7.2.3 and HEAD. |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0x00000020`, `O2=0xFFFFFFFF`. |
| ROCm HEAD with the same PR patches applied locally | Reproduces: `O0=0x00000020`, `O2=0xFFFFFFFF`. |

## Why the fuzzer doesn't see it

* `amdgcn.sffbh` *is* in the fuzzer's emit set as a side effect of the
  signed first-bit-high random op, but the fuzzer never directly pairs
  it with a `umin` to a constant `>= 32`.
* Even if it did, the fold only misbehaves for input distributions that
  put `x = -1` *with non-negligible probability*; the directed fuzzer's
  random i32 inputs hit `-1` only by accident.
* The interpreter oracle is skipped for any module containing an
  `amdgcn.*` intrinsic, so it doesn't catch the mismatch from
  `sffbh(-1) = -1` either.
