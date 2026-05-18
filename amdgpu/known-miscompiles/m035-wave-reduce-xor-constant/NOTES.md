# m035: `wave.reduce.xor` of a constant folded to the unreduced value

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after adding vectorized LLVM intrinsic generation. The original fuzzer
program was much larger, but the active miscompile reduces to a single AMDGPU
wave reduction.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m035-wave-reduce-xor-constant/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
[255] input=0x00000000 O0=0x00000000 O2=0x0000001e mismatch=true
any_mismatch=true
```

## Reduction

The reproducer launches 256 workitems and stores the result of:

```llvm
%reduced = call i32 @llvm.amdgcn.wave.reduce.xor.i32(i32 30, i32 0)
```

The second operand is the documented target-default strategy hint. On `gfx950`
the wavefront size is 64, so XOR-reducing the constant value `30` across every
lane in a full wave should produce `0`. The `-O0` output stores `0`, while the
ROCm 7.2.3 `-O2` output stores the unreduced input value `30`.

## Root Cause Notes

The ROCm 7.2.3 `-O0` assembly preserves the reduction semantics by multiplying
the input by the active-lane parity:

```asm
s_mov_b64 s[4:5], exec
s_bcnt1_i32_b64 s3, s[4:5]
s_and_b32 s3, s3, 1
s_mul_i32 s2, s2, s3
global_store_dword v0, v1, s[0:1]
```

For a full 64-lane wave, the parity is `0`, so the stored result is `0`.

The ROCm 7.2.3 `-O2` assembly folds the intrinsic to the original constant:

```asm
v_mov_b32_e32 v1, 30
global_store_dword v0, v1, s[0:1]
```

This points at an AMDGPU `-O2` constant-fold/combine for
`llvm.amdgcn.wave.reduce.xor` that treats the reduction as an identity operation
instead of accounting for the active wave size.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000000`, `O2=0x0000001e`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Passes: `O0=0x00000000`, `O2=0x00000000`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Passes: `O0=0x00000000`, `O2=0x00000000`. |

Original fuzzer input SHA-1:

```text
b650d2b86853681e86038f25894c47eaea16f6de
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses `llvm.amdgcn.wave.reduce.xor` generation by
default. Set `FUZZX_ALLOW_M035_WAVE_REDUCE_XOR=1` to re-enable it when replaying
the original fuzzer input or intentionally fuzzing this reduction family.
