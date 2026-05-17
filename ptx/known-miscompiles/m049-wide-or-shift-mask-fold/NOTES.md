# m049-wide-or-shift-mask-fold

Found during a focused sweep for newly added register-shift and predicated-ALU
generation. The reduced testcase contains neither of those new instruction
forms; this was a rediscovery because that sweep left 64-bit scratch ALU
generation enabled.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-regshift-predalu-smoke-nkxF7ZGv/div-1779044226-18b06f3bbf605f9c
```

The automated reducer first produced a 119-line testcase that used an
undefined `%rd6` value after removing part of a 64-bit scratch sequence. The
reducer now keeps `%rd6` / `%rd7` scratch lines together enough to avoid that
invalid shape. A follow-up grouped hand pass reduced the valid testcase to the
straight-line PTX in `reduced.ptx`.

## Scalar Trace

For each thread:

```text
input = in[tid]
a = low32(or.b64(cvt.u64.u32(262144), cvt.u64.u32(267548771)))
  = 0x0ff67863
b = tid + 16
c = 0 - b
d = c >> 13
e = a << 19
mask = e + d + d
f = mask & input
out = e + (f - 15029 - 32)
```

For thread 0, `input = 0xd267d34c`, `d = 0x0007ffff`, `e = 0xc3180000`,
and `mask = 0xc327fffe`. The correct stored value is therefore
`0x853f9877`. `ptxas -O0` stores that value; optimized ptxas stores
`0xd37f9877`.

Replacing the `or.b64` / `mov.b64` low-word producer with either
`mov.u32 0x0ff67863` or an equivalent `or.b32` removes the divergence.
Replacing the final 64-bit subtract with `sub.u32` does not remove it. This
points at an optimized fold where a 64-bit OR low word feeds the later
shift/add/and mask expression.

In the optimized SASS, ptxas collapses the mask computation into a `LEA.HI`
sequence before the `LOP3` implementing the `and`:

```text
VIADD       R0, R9, 0x10
IADD3       R7, -R0
SHF.R.U32.HI R7, RZ, 0xd, R7
LEA.HI      R7, -R0, R7, 0xff67863, 0x13
LOP3.LUT    R7, R7, R2, RZ, 0xc0
```

This is probably related to the broad low-word-of-64-bit-operation optimizer
area exposed by `m040-mulwide-neg-shr-fold`, but it is a distinct reduced
shape: this testcase needs `or.b64`, not `mul.wide`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-17 with CUDA Toolkit 13.2.1 nvcc/ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_WIDE_INT=1`. Focused
sweeps for unrelated new generator features should also usually set
`DIV_DISABLE_MUL_WIDE=1` to avoid the neighboring `mul.wide` family.
