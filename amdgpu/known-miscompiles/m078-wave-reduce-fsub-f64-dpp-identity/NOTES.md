# m078: DPP-strategy lowering of `wave.reduce.fsub.f64` returns wrong sign-of-zero (disagrees with iterative strategy and with IEEE)

*Discovery method: code inspection.* Found by reading the wave-reduce
custom-inserter in `llvm/lib/Target/AMDGPU/SIISelLowering.cpp`.

The DPP-strategy lowering of `llvm.amdgcn.wave.reduce.fsub.f64` uses the
generic FP64 identity value `-0.0` (`0x8000000000000000`) produced by
`getIdentityValueForWaveReduction(V_ADD_F64_e64)`, even though the
iterative-strategy lowering explicitly overrides that identity to `+0.0`
(`0x0000000000000000`) for FSUB-of-F64 with the comment "+0.0 for double sub
reduction".  Running the two strategies on the same all-zero input therefore
returns different signs of zero.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m078-wave-reduce-fsub-f64-dpp-identity/reduced.ll
```

The kernel runs both strategies on a per-lane VGPR value of `+0.0`, XORs the
two `i64` bit-patterns of the results, and stores the high 32 bits of the XOR
to `out[tid]`.  If the two strategies agreed, the stored value would be `0`.
The observed result on `gfx950`, wave64:

```text
[0]   input=0x00000000 O0=0x80000000 O2=0x80000000 mismatch=false
...
[255] input=0x00000000 O0=0x80000000 O2=0x80000000 mismatch=false
any_mismatch=false
```

Every active lane stores `0x80000000`, i.e. the two strategies' results
differ in exactly the FP64 sign bit:

* `i32 1` (ITERATIVE) → `+0.0` (bits `0x0000000000000000`)
* `i32 2` (DPP)       → `-0.0` (bits `0x8000000000000000`)

`mismatch=false` between `-O0` and `-O2` is **expected** here: the strategy is
selected by an `immarg`, not by optimization level, so both `-O0` and `-O2`
take the same (buggy) path.  The bug is a strategy-consistency / soundness
issue, not an opt-level miscompile.  The stored value (`0x80000000` instead of
`0x00000000`) is what proves the strategies disagree.

## Root cause

`llvm/lib/Target/AMDGPU/SIISelLowering.cpp`:

* `getIdentityValueForWaveReduction(V_ADD_F64_e64)` returns `0x8000000000000000`
  (`-0.0`).  For FSUB-of-F64 the lowering reuses `V_ADD_F64` with a `NEG` src
  modifier on the second operand, so the **correct** identity for the
  *subtraction* reduction is `+0.0`, not `-0.0`.

* The ITERATIVE branch (line ~6108) acknowledges this and overrides:

  ```cpp
  uint64_t IdentityValue =
      MI.getOpcode() == AMDGPU::WAVE_REDUCE_FSUB_PSEUDO_F64
          ? 0x0 // +0.0 for double sub reduction
          : getIdentityValueForWaveReduction(Opc);
  ```

  Each iteration then computes `acc = -lane + acc`, starting from
  `acc = +0.0`, and for all-zero input ends with `+0.0`.

* The DPP branch (line ~6326) does **not** override:

  ```cpp
  uint64_t IdentityValue = getIdentityValueForWaveReduction(Opc);
  ```

  Inactive lanes are seeded with `-0.0`; active lanes hold `+0.0`.  The
  pairwise DPP add yields `+0.0` (since `+0.0 + +0.0 = +0.0` and
  `+0.0 + -0.0 = +0.0`), so the accumulated lane-63 value is `+0.0`.  The
  final post-DPP negation step (lines ~6620-6633) then computes
  `Identity + (-Final) = -0.0 + (-(+0.0)) = -0.0 + -0.0 = -0.0`, which is
  what `lowerWaveReduce` returns.

## Which sign is "correct"?

For the FSUB reduction `x[0] - x[1] - ... - x[N-1]`:

* Under IEEE-754 round-to-nearest, `0 - 0 - ... - 0 = +0.0`.  Both an
  associative left-fold and a tree-fold of `(+0.0) + (-(+0.0))` over the
  active lanes yield `+0.0`.  The iterative strategy therefore returns the
  IEEE-correct value.

* The DPP strategy ends with an unnecessary final `Identity + (-final)`
  operation that *introduces* a `-0.0` because the identity it uses is `-0.0`
  (the additive-with-rounding identity).  Using `+0.0` as the identity for
  the FSUB lowering (matching the ITERATIVE override) would make the final
  step `+0.0 + (-(+0.0)) = +0.0` and bring the DPP result back into agreement
  with both the iterative result and the IEEE specification.

Note: the SGPR uniform-input path (`lowerWaveReduce`, ~line 6011, the
`V_ADD_F64_e64` arm) computes `neg(srcReg) * active_lane_count` as f64.  For
all-zero input this yields `-0.0 * 64.0 = -0.0`, agreeing with the DPP path
but disagreeing with the iterative path -- so the SGPR path has the same
identity-polarity confusion as DPP.

## Fix sketch

Either:

1. Apply the same `WAVE_REDUCE_FSUB_PSEUDO_F64 ? 0x0 : ...` override at the
   DPP branch (line ~6326) and at the SGPR-uniform `V_ADD_F64` arm, so that
   the inactive-lane / multiply-by-zero identity is `+0.0` and the post-DPP
   negation step does not introduce `-0.0`; *or*

2. Fix `getIdentityValueForWaveReduction(V_ADD_F64_e64)` to return `+0.0`
   instead of `-0.0` (the additive identity in round-to-nearest is `-0.0`,
   but in this codebase that identity is only used for FSUB reductions, where
   `+0.0` is the right value).  This also requires confirming there is no
   user of the `V_ADD_F64`-keyed identity that actually wants `-0.0`.

## Why the fuzzer doesn't see it

* `llvm.amdgcn.wave.reduce.fsub.f64` is currently not in the fuzzer's
  emission set (only the int family is, and only via the constant-folded
  paths that triggered m035/m036).
* The bug does not show up as an `-O0` vs `-O2` divergence because the
  strategy is selected by an `immarg`, not by optimization level.  The
  harness's only existing oracle is "did `-O0` and `-O2` produce the same
  bytes?", so a strategy-vs-strategy disagreement is invisible to it unless
  the reproducer kernel itself xors the two results (as this one does).
* The interpreter oracle is skipped for any module containing an `amdgcn.*`
  intrinsic, so it could not catch the divergence either.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD (local `build/llvm-fuzzer`, ROCm staging clang 23.0.0git) | Reproduces: all 256 active lanes store `0x80000000` (iter vs DPP XOR high half) at both `-O0` and `-O2`. |
