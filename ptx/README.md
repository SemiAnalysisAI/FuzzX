# FuzzX PTX

*This section is human-written.*

This directory contains the FuzzX PTX fuzzer. It looks for correctness bugs in
NVIDIA's PTX toolchain.

Currently it fuzzes `ptxas`.

The process of looking for a bug is:

 - Generate a random PTX program.
 - Compile it with `-O0` and `-O2`.
 - Run both programs.
 - Compare their outputs.

So long as the random program is "legal" (meaning, mostly, it doesn't have
undefined behavior) the output from the two programs should be identical.  If
they are not the same, that indicates a likely miscompile.

Most of the complexity in the fuzzer is around generating random programs.
Obviously we can't generate truly arbitrary programs; they might have UB or
infinite loops.  Perhaps less obvious is that, after we've found one bug, we
need to generate programs that avoid that bug, otherwise we'll just keep
finding it over and over.  So we have many flags that let you disable
particular known-buggy idioms.

Fuzzers like libFuzzer and AFL++ allow you to do "directed" fuzzing, where you
observe the branches taken by the binary under test and steer fuzzing towards
"interesting" inputs.  It is possible to do directed fuzzing on black-box
binaries like `ptxas` using e.g. AFL++'s QEMU mode.  But we don't currently do
this, because we've found that undirected fuzzing is sufficient (for now).

All of the code here is AI-written, using ChatGPT 5.5 and Opus 4.7.  I haven't
read it at all.  Fuzzing is inherently messy, and anyway the goal here is to
find bugs, not to build a beautiful fuzzer.

After finding a miscompile, you'll want to:

 - come up with a minimal testcase,
 - root-cause the bug,
 - write a reproducer to share with the vendor, and
 - add a flag to the fuzzer so it avoids finding the same bug again.

I also use AI for this.  Eventually it writes a reproducer into the
`known-miscompiles` directory.

I've had good luck using `/goal` to get the AI to run the fuzzer, wait for a
bug to appear, process it as above, and then restart the fuzzer.  The biggest
issue seems to be that it's slow at minimizing testcases.

Everything below this line is AI-written slop.  Good luck!

----------

## Requirements

| Component | Notes |
| --- | --- |
| Rust | Uses the toolchain in `rust-toolchain.toml`. |
| CUDA driver + `libcuda` | Required for fuzzing, verification, and reduction. |
| CUDA Toolkit `ptxas` | Set `PTXAS=/path/to/ptxas` for reproducible runs. |
| CUDA Toolkit `nvcc` | Required for the standalone CUDA inline-PTX reproducers. |
| NVIDIA GPU matching `TARGET_ARCH` | `fuzzx-execgen` currently defaults to `sm_103`. |

## Layout

| Path | Purpose |
| --- | --- |
| `crates/fuzzx-execgen` | PTX kernel generator for differential testing. |
| `crates/fuzzx-exec` | `ptxas` compiler wrapper plus CUDA launch/diff helpers. |
| `crates/fuzzx-diff` | Differential fuzzer plus show/verify/reduce helpers. |
| `known-miscompiles/` | Reduced or standalone reproducers for confirmed findings. |
| `scripts/check-gen.sh` | Generator acceptance-rate smoke test against `ptxas`. |

## `ptxas` Bugs Found

Except where otherwise noted, these have been tested on `sm_103` (i.e. B300).

Version | Description |
| --- | --- |
| 13.2.78 | [m001-seed-050f](known-miscompiles/m001-seed-050f/NOTES.md): Uniform loop-latch optimization mishandles divergent loop-header entry. |
| 13.2.78 | [m002-structured-lop3](known-miscompiles/m002-structured-lop3/NOTES.md): `selp` / `lop3` / `xor` fold computes the wrong truth-table result. |
| 13.2.78 | [m003-no-lop3-max-chain](known-miscompiles/m003-no-lop3-max-chain/NOTES.md): `sub.u32` plus `max.s32` chain fold incorrectly includes the pre-subtract value. |
| 13.0.88 | [m051-sat-sub-add-fold](known-miscompiles/m051-sat-sub-add-fold/NOTES.md): `sub.sat.s32` followed by adding back the subtrahend folds as if saturation cannot occur. |
| 13.2.78 | [m004-mulhi-loop-tripcount](known-miscompiles/m004-mulhi-loop-tripcount/NOTES.md): Loop removal drops two `mul.hi.s32` accumulator updates. |
| 13.2.78 | [m005-prmt-ifconvert-mask](known-miscompiles/m005-prmt-ifconvert-mask/NOTES.md): If-converted `prmt.b32` mask fold drops a source operand. |
| 13.2.78 | [m006-ifconvert-not-xor](known-miscompiles/m006-ifconvert-not-xor/NOTES.md): If-converted `not.b32` plus `xor.b32` fold uses the wrong truth table. |
| 13.2.78 | [m007-signed-unsigned-ifconvert](known-miscompiles/m007-signed-unsigned-ifconvert/NOTES.md): Nested if-conversion conflates signed and unsigned predicates. |
| 13.2.78 | [m008-funnel-shift-loop-unroll](known-miscompiles/m008-funnel-shift-loop-unroll/NOTES.md): Loop unroll rewrites a loop-carried `shf.r.wrap.b32` recurrence incorrectly. |
| 13.2.78 | [m009-neg-loop-after-counted-loop](known-miscompiles/m009-neg-loop-after-counted-loop/NOTES.md): Loop deletion stores a pre-`neg.s32` value after counted-loop simplification. |
| 13.2.78 | [m010-shr-s32-range-fold](known-miscompiles/m010-shr-s32-range-fold/NOTES.md): Range fold treats `shr.s32` as if it were unsigned before an unsigned compare. |
| 13.2.78 | [m011-bfind-after-empty-loop](known-miscompiles/m011-bfind-after-empty-loop/NOTES.md): Empty-loop simplification folds a `bfind.u32`-derived value incorrectly. |
| 13.2.78 | [m012-empty-loop-intmax-sub](known-miscompiles/m012-empty-loop-intmax-sub/NOTES.md): Counted empty-loop fold miscomputes an `INT_MAX` subtraction sequence. |
| 13.2.78 | [m048-intmax-popc-sub-mask-fold](known-miscompiles/m048-intmax-popc-sub-mask-fold/NOTES.md): Likely related to m012; structured branch context misfolds a `popc`-derived `INT_MAX` subtract before an `and` mask. |
| 13.2.78 | [m013-set-true-cmp-one](known-miscompiles/m013-set-true-cmp-one/NOTES.md): `set.eq` materialization is folded as a predicate instead of `0xffffffff`. |
| 13.2.78 | [m047-selp-ge-zero-branch-fold](known-miscompiles/m047-selp-ge-zero-branch-fold/NOTES.md): `selp` materialization of `0xffffffff` feeding an unsigned `>= 0` branch fold skips an always-taken arm. |
| 13.2.78 | [m014-vsub4-divergent-branch](known-miscompiles/m014-vsub4-divergent-branch/NOTES.md): `vsub4.u32.u32.u32` constant fold uses the wrong byte-lane intermediate. |
| 13.2.78 | [m015-abs-loop-bmsk-fold](known-miscompiles/m015-abs-loop-bmsk-fold/NOTES.md): Loop deletion uses the pre-`abs.s32` live-out value in a `bmsk` expression. |
| 13.2.78 | [m016-slct-s32-immediate-fold](known-miscompiles/m016-slct-s32-immediate-fold/NOTES.md): `slct.s32.s32` immediate fold selects the wrong arm for a positive value. |
| 13.2.78 | [m017-addc-shift-carry-fold](known-miscompiles/m017-addc-shift-carry-fold/NOTES.md): `add.cc.u32` / `addc.u32` fold injects an incorrect carry-in. |
| 13.2.78 | [m029-addc-mul-carry-fold](known-miscompiles/m029-addc-mul-carry-fold/NOTES.md): Likely same root cause as m017; `addc.u32` fold injects an incorrect carry-in after multiply-derived operands. |
| 13.2.78 | [m018-subc-cnot-shift-borrow-fold](known-miscompiles/m018-subc-cnot-shift-borrow-fold/NOTES.md): `sub.cc.u32` / `subc.u32` fold injects an incorrect borrow-in after `cnot`. |
| 13.2.78 | [m027-subc-shr-mul-borrow-fold](known-miscompiles/m027-subc-shr-mul-borrow-fold/NOTES.md): Likely same root cause as m018; `subc.u32` fold uses the wrong borrow source after shift and multiply. |
| 13.2.78 | [m019-structured-loop-uniform-counter](known-miscompiles/m019-structured-loop-uniform-counter/NOTES.md): Structured loop counters are promoted to uniform state and lose per-lane values. |
| 13.2.78 | [m020-mixed-minmax-signedness-fold](known-miscompiles/m020-mixed-minmax-signedness-fold/NOTES.md): Mixed signed/unsigned `min` / `max` fold drops the runtime input. |
| 13.2.78 | [m021-cnot-funnel-add](known-miscompiles/m021-cnot-funnel-add/NOTES.md): `shf.r.wrap.b32` plus add fold loses part of the shifted value. |
| 13.2.78 | [m022-neg-funnel-left-add](known-miscompiles/m022-neg-funnel-left-add/NOTES.md): `neg.s32` plus `shf.l.wrap.b32` fold produces a sign-extension-shaped error. |
| 13.2.78 | [m023-mul-wide-hi-ice](known-miscompiles/m023-mul-wide-hi-ice/NOTES.md): Optimized compile crashes on a `mul.wide` low-half feeding signed high multiply. |
| 13.2.78 | [m024-prmt-cvt-u16-fold](known-miscompiles/m024-prmt-cvt-u16-fold/NOTES.md): `prmt.b32` plus `cvt.u16` fold drops the permuted source contribution. |
| 13.0.88 | [m055-prmt-reg-control-eq-fold](known-miscompiles/m055-prmt-reg-control-eq-fold/NOTES.md): Register-control `prmt.b32` feeding an equality fold selects the wrong arm. |
| 13.0.88 | [m054-packed-add-cvt-fold](known-miscompiles/m054-packed-add-cvt-fold/NOTES.md): `add.s16x2` feeding `cvt.u16` and another packed add drops the first packed-add contribution. |
| 13.0.88 | [m056-packed-add-cvt-s16-fold](known-miscompiles/m056-packed-add-cvt-s16-fold/NOTES.md): Likely same root cause as m054; `add.u16x2` feeding `cvt.s16` drops the packed-add contribution. |
| 13.0.88 | [m057-s16-unary-intmin-fold](known-miscompiles/m057-s16-unary-intmin-fold/NOTES.md): `abs.s16` / `neg.s16` of `INT16_MIN` feeding `cvt.s32.s16` is treated as a positive value. |
| 13.0.88 | [m058-scalar16-min-cvt-fold](known-miscompiles/m058-scalar16-min-cvt-fold/NOTES.md): Scalar `min.{u16,s16}` through `.b16` scratch registers folds a following equality predicate incorrectly. |
| 13.2.78 | [m025-shl-xor-square-lowbits](known-miscompiles/m025-shl-xor-square-lowbits/NOTES.md): Fold loses the fact that a value is shifted left before testing low bits. |
| 13.2.78 | [m026-shr-abs-ult-fold](known-miscompiles/m026-shr-abs-ult-fold/NOTES.md): Fold reasons about `0 - abs(n)` as signed or non-wrapping before unsigned compare. |
| 13.2.78 | [m028-shf-r-wrap-sub-fold](known-miscompiles/m028-shf-r-wrap-sub-fold/NOTES.md): `shf.r.wrap.b32` output is folded to zero before a final subtract. |
| 13.2.78 | [m030-not-clz-predicate-fold](known-miscompiles/m030-not-clz-predicate-fold/NOTES.md): Guarded path fold drops or misapplies `not.b32` before `clz.b32`. |
| 13.2.78 | [m031-guarded-sub-sub-fold](known-miscompiles/m031-guarded-sub-sub-fold/NOTES.md): Guarded `x - (0x80000000 - x)` fold drops the `2*x` contribution. |
| 13.2.78 | [m032-cnot-neg-ugt-fold](known-miscompiles/m032-cnot-neg-ugt-fold/NOTES.md): `cnot` / `neg` chain feeding an unsigned-greater-than predicate folds to the wrong arm. |
| 13.2.78 | [m046-cnot-underflow-ugt-fold](known-miscompiles/m046-cnot-underflow-ugt-fold/NOTES.md): Likely same root cause as m032; `cnot` feeding wrapped subtraction before an unsigned comparison selects the wrong arm. |
| 13.2.78 | [m033-not-xor-branch-fold](known-miscompiles/m033-not-xor-branch-fold/NOTES.md): Branch-specialized `not` / `xor` path folds the wrong value into the store. |
| 13.2.78 | [m035-xor-not-predicate-fold](known-miscompiles/m035-xor-not-predicate-fold/NOTES.md): Likely same root cause as m033; `xor.b32` by `0xffffffff` feeding a predicate selects the wrong arm. |
| 13.2.78 | [m034-bfind-zero-branch-fold](known-miscompiles/m034-bfind-zero-branch-fold/NOTES.md): Branch fold treats `bfind.u32 0` as `0` instead of `0xffffffff`. |
| 13.2.78 | [m036-mulhi-control-fold](known-miscompiles/m036-mulhi-control-fold/NOTES.md): Control-flow fold around `mul.hi.s32` uses an incorrect folded constant. |
| 13.2.78 | [m037-bmsk-clz-bfi-fold](known-miscompiles/m037-bmsk-clz-bfi-fold/NOTES.md): `bmsk` / `clz` / `bfi` / `mad.lo` value-chain fold sets an extra output bit. |
| 13.2.78 | [m038-structured-empty-else-fold](known-miscompiles/m038-structured-empty-else-fold/NOTES.md): Always-false structured branch with an empty else arm folds as if the untaken then arm executed. |
| 13.2.78 | [m039-else-redefinition-fold](known-miscompiles/m039-else-redefinition-fold/NOTES.md): Branch fold drops the executed else-path redefinition of a value initialized before the branch. |
| 13.2.78 | [m040-mulwide-neg-shr-fold](known-miscompiles/m040-mulwide-neg-shr-fold/NOTES.md): `mul.wide` low word feeding wrapped negation and logical shift loses the shifted high-bit contribution. |
| 13.2.78 | [m049-wide-or-shift-mask-fold](known-miscompiles/m049-wide-or-shift-mask-fold/NOTES.md): Likely related to m040; `or.b64` low word feeding a shift/add mask fold computes the wrong mask. |
| 13.2.78 | [m041-or-shifted-square-fold](known-miscompiles/m041-or-shifted-square-fold/NOTES.md): `or.b32` after a square known to have zero low 32 bits folds with a missing output bit. |
| 13.2.78 | [m044-mul-lo-square-fold](known-miscompiles/m044-mul-lo-square-fold/NOTES.md): Likely same root cause as m041; square of a shifted `mul.lo` value folds to `0x80000000` instead of zero. |
| 13.2.78 | [m042-vsub4-else-ifconvert-fold](known-miscompiles/m042-vsub4-else-ifconvert-fold/NOTES.md): If-converted else arm using `vsub4` computes the wrong value for the one lane that takes it. |
| 13.2.78 | [m043-shr-sub-branch-fold](known-miscompiles/m043-shr-sub-branch-fold/NOTES.md): Branch-sensitive unsigned shift after wrapped subtraction loses the shifted high bit. |
| 13.2.78 | [m050-reg-shl-mask-fold](known-miscompiles/m050-reg-shl-mask-fold/NOTES.md): Masked register-count `shl.b32` chains fold to the wrong shifted value. |
| 13.0.88 | [m052-bfe-reg-pos-fold](known-miscompiles/m052-bfe-reg-pos-fold/NOTES.md): Register-position `bfe.s32` with an out-of-range start folds to the wrong sign-filled value. |
| 13.0.88 | [m053-bfi-reg-len-fold](known-miscompiles/m053-bfi-reg-len-fold/NOTES.md): Likely related to m052; register-length `bfi.b32` preserves high base bits that should be overwritten. |
| 13.2.78 | [m045-brev-branch-fold](known-miscompiles/m045-brev-branch-fold/NOTES.md): Branch-join fold around `brev.b32` computes `0x8000001d` instead of `0x8000001f`. |

## Running

Build the tools:

```bash
cargo build --release -p fuzzx-diff
```

Run a differential sweep:

```bash
target/release/fuzzx-diff \
  --ptxas /usr/local/cuda/bin/ptxas \
  --max-iters 100000
```

Divergences are saved under `DIV_OUT_DIR` (default: `divergences/`) as
directories containing `seed.bin`, `program.ptx`, `input.bin`, `output_o0.*`,
`output_o3.*`, and `summary.txt`.

Useful follow-up commands:

```bash
target/release/fuzzx-diff-show divergences/div-...
target/release/fuzzx-diff-verify divergences/div-...
target/release/fuzzx-diff-reduce divergences/div-...
target/release/fuzzx-diff-test divergences/div-.../program.ptx divergences/div-.../input.bin
target/release/fuzzx-diff-inspect-outputs divergences/div-.../program.ptx divergences/div-.../input.bin
```

Check how often generated PTX assembles:

```bash
PTXAS=/usr/local/cuda/bin/ptxas scripts/check-gen.sh 200
```

## Configuration

`fuzzx-diff` accepts kebab-case CLI flags for the run-control and generator
settings below; `target/release/fuzzx-diff --help` lists the full set. The
same settings can still be supplied as environment variables, which is useful
for long-running scripted sweeps. Boolean environment variables accept `1`,
`true`, `yes`, or `on` for true, and `0`, `false`, `no`, or `off` for false.

### Shared

| Variable | Default | Meaning |
| --- | --- | --- |
| `PTXAS` | `/usr/local/cuda/bin/ptxas`, then `$HOME/bin/ptxas`, then `ptxas` | Target `ptxas` binary. Set this explicitly for reproducible runs. |
| `TMPDIR` | Caller value; some tools use `/dev/shm` when unset and available. | Temporary directory for PTX/cubin files. |

### Run Control

| Variable | Default | Meaning |
| --- | --- | --- |
| `DIV_OUT_DIR` | `divergences` | Directory for saved divergence bundles. |
| `DIV_STARTING_SEED` | nanoseconds since epoch | First seed in the deterministic seed stream. |
| `DIV_MAX_ITERS` | unlimited | Stop after this many generated candidates. |
| `DIV_PRINT_EVERY_SECS` | `5` | Progress-report interval. |
| `DIV_PROGRAM_BYTES` | `4096` | Bytes derived from each seed and consumed by the generator. |
| `DIV_GPUS` | all visible CUDA devices | Comma-separated CUDA device ordinals, for example `0,1,2`. |
| `DIV_WORKERS_PER_GPU` | `16` | Worker threads per selected GPU. |

### Generator Shape

| Variable | Default | Meaning |
| --- | --- | --- |
| `DIV_STRUCTURED_CONTROL_FLOW` | `false` | Use structured single-entry if/loop generation instead of arbitrary CFG generation. |
| `DIV_MIN_BLOCKS` / `DIV_MAX_BLOCKS` | `1` / `10` | Block-count bounds. |
| `DIV_MIN_INSTS_PER_BLOCK` / `DIV_MAX_INSTS_PER_BLOCK` | `1` / `6` | Instruction-count bounds per block. |
| `DIV_WORKING_REGS` | `8` | Number of working `u32` registers. |
| `DIV_MAX_LOOP_ITERS` | `16` | Maximum generated loop-trip count. |
| `DIV_MAX_IMMEDIATE` | `32` | Maximum ordinary immediate value. |
| `DIV_MAX_STRUCTURED_DEPTH` | `3` | Maximum nesting depth for structured control flow. |

### Generator Feature Toggles

All variables in this table default to `false`; setting one to true suppresses
that feature.

| Variable | Suppresses |
| --- | --- |
| `DIV_DISABLE_STRUCTURED_LOOPS` | Counted-loop shapes in structured mode. |
| `DIV_DISABLE_ARBITRARY_LOOPS` | Backedge loop terminators in arbitrary CFG mode. |
| `DIV_DISABLE_LOP3` | `lop3.b32`. |
| `DIV_DISABLE_PREDICATED_LOP3` | Predicated `lop3.b32` instructions. |
| `DIV_DISABLE_MINMAX` | `min.u32`, `max.u32`, `min.s32`, `max.s32`. |
| `DIV_DISABLE_SELP` | `selp.b32`. |
| `DIV_DISABLE_SUB` | Random `sub.u32` ALU instructions. |
| `DIV_DISABLE_MUL_LO` | `mul.lo.u32` and `mad.lo.u32`. |
| `DIV_DISABLE_SIGNED_LO_ALU` | Signed low-ALU spellings, including saturating add/sub. |
| `DIV_DISABLE_SAT_ARITH` | `add.sat.s32` and `sub.sat.s32`. |
| `DIV_DISABLE_PACKED_ADD` | `add.u16x2` and `add.s16x2`. |
| `DIV_DISABLE_SIGNED_PACKED_ADD` | `add.s16x2` only. |
| `DIV_DISABLE_PREDICATED_PACKED_ADD` | Predicated `add.u16x2` and `add.s16x2` instructions. |
| `DIV_DISABLE_PACKED_MINMAX` | `min/max.{u16x2,s16x2}`. |
| `DIV_DISABLE_SIGNED_PACKED_MINMAX` | `min/max.s16x2` only. |
| `DIV_DISABLE_PREDICATED_PACKED_MINMAX` | Predicated `min/max.{u16x2,s16x2}` instructions. |
| `DIV_DISABLE_SCALAR_16BIT` | Scalar 16-bit ALU through `.b16` scratch registers. |
| `DIV_DISABLE_SIGNED_SCALAR_16BIT` | Signed scalar 16-bit ALU while retaining unsigned `u16` ops. |
| `DIV_DISABLE_SCALAR_16BIT_MIN` | `min.u16` and `min.s16` while retaining scalar 16-bit max and arithmetic instructions. |
| `DIV_DISABLE_SCALAR_16BIT_SIGNED_UNARY` | `abs.s16` and `neg.s16` while retaining other scalar 16-bit ALU instructions. |
| `DIV_DISABLE_SCALAR_16BIT_BITWISE` | `and.b16`, `or.b16`, `xor.b16`, and `not.b16`. |
| `DIV_DISABLE_SCALAR_16BIT_SHIFTS` | `shl.b16`, `shr.u16`, and `shr.s16` with immediate counts in `0..15`. |
| `DIV_DISABLE_SCALAR_16BIT_COMPARE` | Scalar 16-bit `setp` and `set` through `.b16` scratch registers. |
| `DIV_DISABLE_SCALAR_16BIT_SELP` | Scalar 16-bit `selp.u16` and `selp.s16`; also disabled by `DIV_DISABLE_SCALAR_16BIT_COMPARE`. |
| `DIV_DISABLE_PREDICATED_SCALAR_16BIT` | Predicated scalar 16-bit ALU instructions. |
| `DIV_DISABLE_MULHI` | `mul.hi.u32` and `mul.hi.s32`. |
| `DIV_DISABLE_SIGNED_MULHI` | `mul.hi.s32` only. |
| `DIV_DISABLE_MAD_HI` | `mad.hi.u32` and `mad.hi.s32`. |
| `DIV_DISABLE_SIGNED_MAD_HI` | `mad.hi.s32` only. |
| `DIV_DISABLE_BITWISE_BINOPS` | `and.b32`, `or.b32`, `xor.b32`. |
| `DIV_DISABLE_OR` | `or.b32` while retaining `and.b32` and `xor.b32`. |
| `DIV_DISABLE_XOR` | `xor.b32` while retaining `and.b32` and `or.b32`. |
| `DIV_DISABLE_PRMT` | `prmt.b32`. |
| `DIV_DISABLE_PREDICATED_PRMT` | Predicated `prmt.b32` instructions. |
| `DIV_DISABLE_REG_PRMT` | Register-control `prmt.b32` instructions. |
| `DIV_DISABLE_PREDICATED_REG_PRMT` | Predicated register-control `prmt.b32` instructions. |
| `DIV_DISABLE_PRMT_MODES` | `prmt.b32` mode variants such as `.f4e`, `.ecl`, and `.rc16`. |
| `DIV_DISABLE_NOT` | `not.b32` and xor-by-`0xffffffff` forms. |
| `DIV_DISABLE_CLZ` | `clz.b32`. |
| `DIV_DISABLE_BREV` | `brev.b32`. |
| `DIV_DISABLE_CNOT` | `cnot.b32`. |
| `DIV_DISABLE_POPC` | `popc.b32`. |
| `DIV_DISABLE_ABS` | `abs.s32`. |
| `DIV_DISABLE_SPECIAL_REGS` | Deterministic special-register reads such as `%laneid` and `%lanemask_*`. |
| `DIV_DISABLE_PREDICATED_SPECIAL_REGS` | Predicated deterministic special-register reads. |
| `DIV_DISABLE_GLOBAL_LOADS` | Bounded read-only `ld.global.{u8,s8,u16,s16,u32,u64,s64}` loads from the input buffer. |
| `DIV_DISABLE_GLOBAL_STORE_ROUNDTRIPS` | Per-thread `st.global.{u8,u16,u32,u64}` plus `ld.global.{u8,s8,u16,s16,u32,u64,s64}` roundtrips through the output buffer. |
| `DIV_DISABLE_CONST_MEMORY` | Bounded read-only `ld.const.{u8,s8,u16,s16,u32,u64,s64}` loads from a module-scope constant buffer. |
| `DIV_DISABLE_LOCAL_MEMORY` | Bounded private local-memory store/load roundtrips, including 64-bit forms. |
| `DIV_DISABLE_SHARED_MEMORY` | Race-free per-thread shared-memory store/load roundtrips, including 64-bit forms. |
| `DIV_DISABLE_PREDICATED_MEMORY` | Predicated forms of bounded memory loads and store/load roundtrips. |
| `DIV_DISABLE_VECTOR_MEMORY` | Aligned `v2`/`v4` u32 memory loads and store/load roundtrips. |
| `DIV_DISABLE_WIDE_MEMORY` | Scalar 64-bit memory loads and store/load roundtrips. |
| `DIV_DISABLE_F32_ARITH` | Sanitized `add/sub/mul/div/fma/copysign/min/max.f32` arithmetic, including approximate f32 division, f32 `.sat` arithmetic, and `.ftz` min/max. |
| `DIV_DISABLE_F32_ROUNDING` | Sanitized non-default rounding and `.ftz` f32 add/sub/mul/div/fma arithmetic. |
| `DIV_DISABLE_F32_UNARY` | Sanitized `abs/neg.f32`, including `.ftz` forms. |
| `DIV_DISABLE_F32_CVT` | Sanitized signed/unsigned 32/64-bit f32/int, saturating f32-to-int, f64-to-f32, and `.ftz` conversion chains. |
| `DIV_DISABLE_F32_SPECIAL_MATH` | Sanitized rounded and `.ftz` f32 `sqrt`/`rcp` plus approx `rcp`, `rsqrt`, `ex2`, `lg2`, `sin`, and `cos`. |
| `DIV_DISABLE_F32_COMPARE` | Sanitized ordered/unordered `set.*.u32.f32` and `setp.*.f32` comparisons, including `.ftz` forms, plus `testp.*.f32` classification. |
| `DIV_DISABLE_F32_SELP` | Sanitized `setp.*.f32`, including `.ftz` forms, feeding `selp.f32`. |
| `DIV_DISABLE_F64_ARITH` | Sanitized `add/sub/mul/div/fma/copysign/min/max.f64` arithmetic. |
| `DIV_DISABLE_F64_ROUNDING` | Sanitized `.rz/.rm/.rp` f64 add/sub/mul/div/fma arithmetic. |
| `DIV_DISABLE_F64_UNARY` | Sanitized `abs.f64` and `neg.f64`. |
| `DIV_DISABLE_F64_CVT` | Sanitized signed/unsigned 32/64-bit f64/int, saturating f64-to-int, and f32-to-f64 conversion chains. |
| `DIV_DISABLE_F64_SPECIAL_MATH` | Sanitized rounded f64 `sqrt` and `rcp`. |
| `DIV_DISABLE_F64_COMPARE` | Sanitized ordered/unordered `set.*.u32.f64` and `setp.*.f64` comparisons plus `testp.*.f64` classification. |
| `DIV_DISABLE_F64_SELP` | Sanitized `setp.*.f64` feeding `selp.f64`. |
| `DIV_DISABLE_SIGNED_CMP` | Signed predicate comparisons. |
| `DIV_DISABLE_SIGNED_DIVREM` | `div.s32` and `rem.s32`. |
| `DIV_DISABLE_REG_DIVREM` | Register-divisor `div.u32` and `rem.u32` with sanitized divisors. |
| `DIV_DISABLE_PREDICATED_REG_DIVREM` | Predicated register-divisor `div.u32` and `rem.u32`. |
| `DIV_DISABLE_PREDICATED_DIVREM` | Predicated `div` and `rem` instructions. |
| `DIV_DISABLE_FUNNEL` | `shf.{l,r}.{wrap,clamp}.b32`. |
| `DIV_DISABLE_REG_FUNNEL` | Register-count `shf.{l,r}.{wrap,clamp}.b32`. |
| `DIV_DISABLE_PREDICATED_FUNNEL` | Predicated `shf.{l,r}.{wrap,clamp}.b32`. |
| `DIV_DISABLE_FUNNEL_CLAMP` | `shf.l.clamp.b32` and `shf.r.clamp.b32`. |
| `DIV_DISABLE_NEG` | `neg.s32`. |
| `DIV_DISABLE_SHL` | `shl.b32`. |
| `DIV_DISABLE_SHR` | `shr.u32`. |
| `DIV_DISABLE_SIGNED_SHR` | `shr.s32`. |
| `DIV_DISABLE_REG_SHIFTS` | Masked register-count shifts. |
| `DIV_DISABLE_PREDICATED_SHIFTS` | Predicated immediate-count shifts. |
| `DIV_DISABLE_PREDICATED_REG_SHIFTS` | Predicated masked register-count shifts. |
| `DIV_DISABLE_BFIND` | `bfind` and `bfind.shiftamt` instructions. |
| `DIV_DISABLE_SIGNED_BFIND` | `bfind.s32` and `bfind.shiftamt.s32`. |
| `DIV_DISABLE_WIDE_BFIND` | 64-bit-source `bfind` and `bfind.shiftamt` instructions. |
| `DIV_DISABLE_SIGNED_WIDE_BFIND` | `bfind.s64` and `bfind.shiftamt.s64`. |
| `DIV_DISABLE_PREDICATED_BFIND` | Predicated `bfind` and `bfind.shiftamt` instructions. |
| `DIV_DISABLE_PREDICATED_WIDE_BFIND` | Predicated 64-bit-source `bfind` and `bfind.shiftamt` instructions. |
| `DIV_DISABLE_FNS` | `fns.b32`. |
| `DIV_DISABLE_REG_FNS` | `fns.b32` with a sanitized register base or offset operand. |
| `DIV_DISABLE_PREDICATED_FNS` | Predicated `fns.b32` instructions. |
| `DIV_DISABLE_PREDICATED_REG_FNS` | Predicated `fns.b32` instructions with a sanitized register base or offset operand. |
| `DIV_DISABLE_BFI` | `bfi.b32`. |
| `DIV_DISABLE_BFE` | `bfe.{u32,s32}`. |
| `DIV_DISABLE_BMSK` | `bmsk.{clamp,wrap}.b32`. |
| `DIV_DISABLE_BMSK_WRAP` | `bmsk.wrap.b32`. |
| `DIV_DISABLE_PREDICATED_BITFIELD` | Predicated `bfe`, `bfi`, and `bmsk` instructions. |
| `DIV_DISABLE_REG_BITFIELD` | Register pos/len operands for `bfe`, `bfi`, and `bmsk`. |
| `DIV_DISABLE_PREDICATED_REG_BITFIELD` | Predicated `bfe`, `bfi`, and `bmsk` instructions with register pos/len operands. |
| `DIV_DISABLE_WIDE_BFE` | 64-bit scratch-register `bfe.{u64,s64}` instructions. |
| `DIV_DISABLE_SIGNED_WIDE_BFE` | 64-bit scratch-register `bfe.s64` instructions. |
| `DIV_DISABLE_WIDE_BFI` | 64-bit scratch-register `bfi.b64` instructions. |
| `DIV_DISABLE_PREDICATED_WIDE_BITFIELD` | Predicated 64-bit scratch-register `bfe` and `bfi` instructions. |
| `DIV_DISABLE_REG_WIDE_BITFIELD` | Sanitized register pos/len operands for 64-bit scratch-register `bfe` and `bfi`. |
| `DIV_DISABLE_PREDICATED_REG_WIDE_BITFIELD` | Predicated 64-bit scratch-register `bfe` and `bfi` instructions with register pos/len operands. |
| `DIV_DISABLE_MAD24` | `mad24.lo.u32` and `mad24.hi.u32`. |
| `DIV_DISABLE_MUL24` | `mul24.{lo,hi}.{u32,s32}`. |
| `DIV_DISABLE_PREDICATED_24BIT` | Predicated `mad24` and `mul24` instructions. |
| `DIV_DISABLE_SUBWORD_WIDE` | 16-bit-source `mul.wide` and `mad.wide` through `.b16` scratch registers. |
| `DIV_DISABLE_SIGNED_SUBWORD_WIDE` | Signed 16-bit-source `mul.wide.s16` and `mad.wide.s16`. |
| `DIV_DISABLE_PREDICATED_SUBWORD_WIDE` | Predicated 16-bit-source `mul.wide` and `mad.wide` instructions. |
| `DIV_DISABLE_MUL_WIDE` | `mul.wide.{u32,s32}`. |
| `DIV_DISABLE_PREDICATED_MUL_WIDE` | Predicated `mul.wide.{u32,s32}` instructions. |
| `DIV_DISABLE_MAD_WIDE` | `mad.wide.{u32,s32}`. |
| `DIV_DISABLE_SIGNED_MAD_WIDE` | `mad.wide.s32`. |
| `DIV_DISABLE_PREDICATED_MAD_WIDE` | Predicated `mad.wide.{u32,s32}` instructions. |
| `DIV_DISABLE_WIDE_HIGH_RESULT` | High-half extraction from `mul.wide` and `mad.wide` results. |
| `DIV_DISABLE_WIDE_INT` | 64-bit scratch-register ALU generation. |
| `DIV_DISABLE_WIDE_MINMAX` | 64-bit scratch-register `min/max.{u64,s64}` instructions. |
| `DIV_DISABLE_WIDE_MULHI` | 64-bit scratch-register `mul.hi.{u64,s64}` instructions. |
| `DIV_DISABLE_PREDICATED_WIDE_INT` | Predicated 64-bit scratch-register ALU generation. |
| `DIV_DISABLE_WIDE_MAD64` | 64-bit operand `mad.{lo,hi}.{u64,s64}` instructions. |
| `DIV_DISABLE_SIGNED_WIDE_MAD64` | 64-bit operand `mad.{lo,hi}.s64` instructions. |
| `DIV_DISABLE_PREDICATED_WIDE_MAD64` | Predicated 64-bit operand `mad` instructions. |
| `DIV_DISABLE_WIDE_SET` | 64-bit scratch-register `set.{cmp}.u32.{u64,s64}` materialization. |
| `DIV_DISABLE_PREDICATED_WIDE_SET` | Predicated 64-bit scratch-register `set` materialization. |
| `DIV_DISABLE_WIDE_SETP` | 64-bit scratch-register `setp`-fed guarded ALU instructions. |
| `DIV_DISABLE_WIDE_SETP_BOOL` | 64-bit scratch-register `setp.<cmp>.<and|or|xor>`-fed guarded ALU instructions. |
| `DIV_DISABLE_WIDE_SELP` | 64-bit scratch-register `selp.b64` instructions. |
| `DIV_DISABLE_WIDE_UNARY` | 64-bit scratch-register `not`, `cnot`, `popc`, `clz`, and `brev` instructions. |
| `DIV_DISABLE_PREDICATED_WIDE_UNARY` | Predicated 64-bit scratch-register unary instructions. |
| `DIV_DISABLE_WIDE_SHIFTS` | 64-bit scratch-register shifts. |
| `DIV_DISABLE_WIDE_REG_SHIFTS` | Masked register-count 64-bit scratch-register shifts. |
| `DIV_DISABLE_PREDICATED_WIDE_SHIFTS` | Predicated 64-bit scratch-register shifts. |
| `DIV_DISABLE_PREDICATED_WIDE_REG_SHIFTS` | Predicated masked register-count 64-bit scratch-register shifts. |
| `DIV_DISABLE_WIDE_DIVREM` | 64-bit scratch-register `div/rem.{u64,s64}` instructions. |
| `DIV_DISABLE_SIGNED_WIDE_DIVREM` | 64-bit scratch-register `div/rem.s64` instructions. |
| `DIV_DISABLE_REG_WIDE_DIVREM` | Register-divisor 64-bit scratch-register `div/rem.{u64,s64}` instructions with sanitized divisors. |
| `DIV_DISABLE_PREDICATED_REG_WIDE_DIVREM` | Predicated register-divisor 64-bit scratch-register `div/rem` instructions. |
| `DIV_DISABLE_PREDICATED_WIDE_DIVREM` | Predicated 64-bit scratch-register `div/rem` instructions. |
| `DIV_DISABLE_WIDE_ADDC` | 64-bit scratch-register `add.cc.u64` / `addc.u64` pairs. |
| `DIV_DISABLE_WIDE_SUBC` | 64-bit scratch-register `sub.cc.u64` / `subc.u64` pairs. |
| `DIV_DISABLE_PREDICATED_WIDE_CARRY` | Predicated 64-bit scratch-register carry pairs. |
| `DIV_DISABLE_WIDE_CARRY_CHAIN` | Three-instruction 64-bit scratch-register carry chains using `addc.cc.u64` or `subc.cc.u64`. |
| `DIV_DISABLE_PREDICATED_WIDE_CARRY_CHAIN` | Predicated three-instruction 64-bit scratch-register carry chains. |
| `DIV_DISABLE_ADDC` | `add.cc.u32` / `addc.u32` pairs. |
| `DIV_DISABLE_SUBC` | `sub.cc.u32` / `subc.u32` pairs. |
| `DIV_DISABLE_PREDICATED_CARRY` | Predicated `add.cc` / `addc` and `sub.cc` / `subc` pairs. |
| `DIV_DISABLE_CARRY_CHAIN` | Three-instruction `add/sub.cc` plus `addc/subc.cc` carry chains. |
| `DIV_DISABLE_PREDICATED_CARRY_CHAIN` | Predicated three-instruction `add/sub` carry chains. |
| `DIV_DISABLE_I32_BOUNDARY_IMMS` | Immediate `0x7fffffff` / `0x80000000` generation. |
| `DIV_DISABLE_DP4A` | `dp4a.{u32,s32}.{u32,s32}`. |
| `DIV_DISABLE_DP2A` | `dp2a.{lo,hi}.{u32,s32}.{u32,s32}`. |
| `DIV_DISABLE_NEGATED_PREDICATES` | Negated `@!%p` instruction predicates. |
| `DIV_DISABLE_PREDICATED_ALU` | Predicated integer ALU and floating-point arithmetic instructions. |
| `DIV_DISABLE_PREDICATED_UNARY` | Predicated integer unary, floating-point unary, and floating-point special-math instructions. |
| `DIV_DISABLE_CVT` | Direct base `cvt.{u32,s32}.{u8,u16,s8,s16}` instructions; narrow and wide round-trips have separate flags. |
| `DIV_DISABLE_PREDICATED_CVT` | Predicated integer and floating-point `cvt` instructions. |
| `DIV_DISABLE_NARROW_CVT` | Narrow `cvt` round-trips through 8/16-bit destination types. |
| `DIV_DISABLE_SIGNED_NARROW_CVT` | Signed narrow `cvt` round-trips. |
| `DIV_DISABLE_PREDICATED_NARROW_CVT` | Predicated narrow `cvt` round-trips. |
| `DIV_DISABLE_WIDE_CVT` | 64-bit-source `cvt` round-trips. |
| `DIV_DISABLE_SIGNED_WIDE_CVT` | Signed 64-bit-source `cvt` round-trips. |
| `DIV_DISABLE_PREDICATED_WIDE_CVT` | Predicated 64-bit-source `cvt` round-trips. |
| `DIV_DISABLE_SZEXT` | `szext.{wrap,clamp}.{u32,s32}`. |
| `DIV_DISABLE_SIGNED_SZEXT` | `szext.*.s32`. |
| `DIV_DISABLE_PREDICATED_SZEXT` | Predicated `szext` instructions. |
| `DIV_DISABLE_SETP_BOOL` | Integer and floating `setp.<cmp>.{and,or,xor}` predicate-combiner instructions. |
| `DIV_DISABLE_SETP_DUAL` | `setp.<cmp> %p|%q` complement-predicate instructions. |
| `DIV_DISABLE_PRED_LOGIC` | `and.pred`, `or.pred`, `xor.pred`, and `not.pred`. |
| `DIV_DISABLE_PREDICATED_MAD` | Predicated `mad.lo.{u32,s32}` instructions. |
| `DIV_DISABLE_PREDICATED_MAD_HI` | Predicated `mad.hi.{u32,s32}` instructions. |
| `DIV_DISABLE_MAD_CARRY` | Three-instruction `mad.cc` / `madc.cc` / `madc` carry chains. |
| `DIV_DISABLE_SIGNED_MAD_CARRY` | Signed `mad.cc` / `madc.cc` / `madc` carry chains. |
| `DIV_DISABLE_PREDICATED_MAD_CARRY` | Predicated `mad.cc` / `madc.cc` / `madc` carry chains. |
| `DIV_DISABLE_PREDICATED_SET` | Predicated integer and floating-point `set`, `setp`, and `testp` instructions. |
| `DIV_DISABLE_PREDICATED_SELP` | Instruction-predicated `selp.b32`, `selp.f32`, and `selp.f64` instructions. |
| `DIV_DISABLE_SAD` | `sad.{u32,s32}`. |
| `DIV_DISABLE_SLCT` | `slct.{u32,s32,b32}.s32`. |
| `DIV_DISABLE_PREDICATED_SAD` | Predicated `sad.{u32,s32}` instructions. |
| `DIV_DISABLE_PREDICATED_SLCT` | Predicated `slct` instructions. |
| `DIV_DISABLE_PREDICATED_DP` | Predicated `dp4a` and `dp2a` instructions. |
| `DIV_DISABLE_PREDICATED_VIDEO` | Predicated video instructions. |
| `DIV_DISABLE_SET` | `set.{cmp}.u32.{u32,s32}`. |
| `DIV_DISABLE_S32_SLCT` | `slct.s32.s32`. |
| `DIV_DISABLE_VIDEO` | PTX video instructions. |
| `DIV_DISABLE_VSUB4` | `vsub4.u32.u32.u32`. |

### Reduction And Sweeping

| Variable | Default | Meaning |
| --- | --- | --- |
| `REDUCE_GPUS` | `DIV_GPUS`, then all visible devices | CUDA devices used by `fuzzx-diff-reduce`. |
| `REDUCE_WORKERS_PER_GPU` | `DIV_WORKERS_PER_GPU`, then host-core based default capped at `16` | Reducer worker count per GPU. |
| `REDUCE_NO_PROGRESS_SECS` | `120` | Reducer timeout when no candidate completes. |
| `DIV_HANG_SECS` | `4` | `fuzzx-diff-sweep` no-progress threshold before reporting hangs. |

## License

FuzzX is licensed under the Apache License, Version 2.0. See [LICENSE](../LICENSE).
