# m017: ROCm 7.2.3 `-O0` drops a vector lane clear before xor

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m017-vector-and-lane0-clear-xor/reduced.ll
```

With ROCm 7.2.3 LLVM on `gfx950`, the reduced testcase produces:

```text
input=0x00000000
O0=0x00000010
O2=0x00000000
mismatch=true
```

## Root Cause Notes

The original fuzzer finding was
`amdgpu/findings/rocm-7.2.3-cov-release-combined-sync1/cxx-diff-1778950776-1186967`.
A second candidate,
`amdgpu/findings/rocm-7.2.3-cov-release-combined-sync1/cxx-diff-1778950701-1183227`,
reduced to the same issue.

The IR is fully defined. It uses fixed-width integer `xor` / `and`, a
non-poison `zeroinitializer` vector, and an in-range `extractelement`.

For workitem 0:

```llvm
%a = xor i32 0, 16          ; 16
%masked = and i32 %a, 18    ; 16
%vand = and <1 x i32> <i32 %masked>, <i32 16>
%e = extractelement <1 x i32> %vand, i32 0  ; 16
%r = xor i32 %masked, %e                   ; 0
```

ROCm 7.2.3 `-O0` stores `%masked` instead. In the generated assembly, the body
computes `%masked` with one `v_bitop3_b32` and immediately stores it:

```asm
s_mov_b32 s1, 18
s_mov_b32 s0, 16
v_mov_b32_e32 v3, s1
v_bitop3_b32 v2, v2, s0, v3 bitop3:0xac
global_store_dword v[0:1], v2, off
```

The missing operation is the clear of bit 4 contributed by `%e`. `-O2`
simplifies the expression to `tid & 2`, which returns zero for the recorded
input.

## Checked Toolchains

Checked on 2026-05-16 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 package, source revision `f58b06dce1f9c15707c5f808fd002e18c2accf7e` | Reproduces. |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces. |
| LLVM HEAD `10756d32f96154f0889eda159ea9a26bc4188bda` | Does not reproduce. |
| ROCm HEAD `9115c466b3577830455f70c4f492429bf6c64b25` | Does not reproduce. |

Original fuzzer input SHA-1:
`4e1b2ee13998544cc60659b6ba0523f663eccddf`.

## Fuzzer Suppression

The directed C++ fuzzer suppresses generated vector lane-0 `and` /
`extractelement` clear-xor shapes by default after this finding. Set
`FUZZX_ALLOW_M017_VECTOR_AND_LANE0_CLEAR_XOR=1` to re-enable this class when
replaying old fuzzer inputs.
