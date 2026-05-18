# m036: `wave.reduce.add` of a constant folded to the unreduced value

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after adding dedicated loop-nest CFG generation. The original generated
program contained a larger branch cascade, but the active miscompile reduces to
a single AMDGPU wave reduction.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m036-wave-reduce-add-constant/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
[255] input=0x00000000 O0=0x00400000 O2=0x00010000 mismatch=true
any_mismatch=true
```

## Reduction

The reproducer launches 256 workitems and stores the result of:

```llvm
%reduced = call i32 @llvm.amdgcn.wave.reduce.add.i32(i32 65536, i32 1)
```

The second operand selects the documented iterative strategy. On `gfx950` the
wavefront size is 64, so adding the constant value `65536` across every lane in
a full wave should produce `0x00400000`. The ROCm 7.2.3 `-O0` output stores
that full reduction, while the ROCm 7.2.3 `-O2` output stores the unreduced
single-lane value `0x00010000`.

## Root Cause Notes

The ROCm 7.2.3 `-O0` assembly preserves the reduction semantics by multiplying
the input by the active-lane count:

```asm
s_mov_b32 s2, 0x10000
s_mov_b64 s[4:5], exec
s_bcnt1_i32_b64 s3, s[4:5]
s_mul_i32 s2, s2, s3
global_store_dword v0, v1, s[0:1]
```

For a full 64-lane wave, the stored result is `64 * 0x10000 == 0x00400000`.

The ROCm 7.2.3 `-O2` assembly folds the intrinsic to the original constant:

```asm
v_mov_b32_e32 v1, 0x10000
global_store_dword v0, v1, s[0:1]
```

This points at the same class of AMDGPU `-O2` constant-fold/combine bug as
m035, but for `llvm.amdgcn.wave.reduce.add`: the combine treats the reduction
as an identity operation instead of accounting for the active wave size.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00400000`, `O2=0x00010000`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Passes: `O0=0x00400000`, `O2=0x00400000`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Passes: `O0=0x00400000`, `O2=0x00400000`. |

Original fuzzer input SHA-1:

```text
0bef5202891d329c6ccc2742d09ca8302af10d0a
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses `llvm.amdgcn.wave.reduce.add` generation by
default. Set `FUZZX_ALLOW_M036_WAVE_REDUCE_ADD=1` to re-enable it when replaying
the original fuzzer input or intentionally fuzzing this reduction family.
