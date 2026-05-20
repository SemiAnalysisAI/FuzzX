# m074: `amdgcn.fmed3(x, y, NaN)` with IEEE-off mode constant-folds to `maximumnum` instead of `minimumnum`

Found by reading `AMDGPUInstCombineIntrinsic.cpp`.  The InstCombine fold
for `amdgcn.fmed3` enumerates which of the three operands is a known NaN
/ infinity, then rewrites the call into a two-operand `min/max` of the
remaining operands.  The polarity used for the third-operand (`Src2`)
case is inverted relative to the comment table that precedes it.

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m074-fmed3-nan-ieee-off-maxmin/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches, `gfx950`:

```text
input=0x40000000   ; lane 0 stores its own value, the other lane stores
                  ; the same packed pair, so both report O0/O2 the same way
O0=0x40000000     ; correct: min(2.0, 3.0) = 2.0
O2=0x40400000     ; wrong: 3.0 (max instead of min)
mismatch=true
```

## Root Cause

The relevant fragment (`AMDGPUInstCombineIntrinsic.cpp`, around line 1535
in current HEAD):

```cpp
} else if ((match(Src2, m_APFloat(ConstSrc2)) &&
            (ConstSrc2->isNaN() || ConstSrc2->isInfinity())) ||
           isa<UndefValue>(Src2)) {
  switch (fpenvIEEEMode(II)) {
  case KnownIEEEMode::On: ...
  case KnownIEEEMode::Off:
    V = (ConstSrc2 && ConstSrc2->isNegInfinity())
            ? IC.Builder.CreateMinimumNum(Src0, Src1)
            : IC.Builder.CreateMaximumNum(Src0, Src1);
    break;
```

Compare against the comment table above that block:

```text
// ieee=0
// s2 _nan: min(s0, s1)
// s2 +inf: max(s0, s1)
// s2 -inf: min(s0, s1)
```

The table says `Min` whenever `Src2` is either a NaN or `-inf`, and `Max`
only for `+inf`.  But the code only treats `-inf` as "Min" and defaults
everything else (`+inf`, `NaN`, `undef`) to `Max`.  For a NaN `Src2` the
generated code is therefore `MaximumNum(Src0, Src1)` instead of
`MinimumNum(Src0, Src1)`.

The parallel arms for `Src0` and `Src1` (a few lines above) use `Min` as
the default and `Max` only for `+inf`, matching the table:

```cpp
case KnownIEEEMode::Off:
  V = IsPosInfinity ? IC.Builder.CreateMaximumNum(Src1, Src2)
                    : IC.Builder.CreateMinimumNum(Src1, Src2);
```

So the bug is asymmetric: `Src0` NaN and `Src1` NaN are folded correctly,
but `Src2` NaN is folded to the opposite extremum.

## Fix sketch

Replace the `Src2`/`IEEE::Off` arm with the same polarity used for
`Src0`/`Src1`:

```cpp
case KnownIEEEMode::Off:
  V = (ConstSrc2 && ConstSrc2->isPosInfinity())
          ? IC.Builder.CreateMaximumNum(Src0, Src1)
          : IC.Builder.CreateMinimumNum(Src0, Src1);
  break;
```

After the fix:
* `Src2 = -inf` → `MinimumNum` (still correct).
* `Src2 = NaN`  → `MinimumNum` (newly correct).
* `Src2 = +inf` → `MaximumNum` (still correct).
* `Src2 = undef`→ `MinimumNum` (was `MaximumNum`; either is defensible
  since undef may take any value, but matches the `Src0`/`Src1` arms).

## Why the fuzzer doesn't see it

* `amdgcn.fmed3` *is* in the fuzzer's emit set, but it's only ever called
  with runtime values, never constants -- and the bug specifically
  requires `Src2` to be a constant NaN.
* The InstCombine fold is gated by `fpenvIEEEMode(II)`, which only
  returns `Off` when the kernel is compiled with the `amdgpu-ieee=false`
  attribute (or equivalent flags).  The fuzzer's emitted kernels all
  inherit the default `amdgpu-ieee=true`.  Even if the fuzzer fed in a
  constant NaN, it would hit the (correct) `KnownIEEEMode::On` arm.
* The interpreter oracle is skipped for any module containing an
  `amdgcn.*` intrinsic, so it can't catch the divergence either.

The three things together (constant NaN third operand, IEEE-off kernel
attribute, AMDGPU intrinsic that disables the interpreter oracle) put
this miscompile in the fuzzer's blind spot.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0x40000000`, `O2=0x40400000`. |
| ROCm 7.2.3 source build | Reproduces: `O0=0x40000000`, `O2=0x40400000`. |
| ROCm HEAD with the same PR patches applied locally | Reproduces: `O0=0x40000000`, `O2=0x40400000`. |
