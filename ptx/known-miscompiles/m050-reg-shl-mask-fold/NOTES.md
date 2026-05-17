# m050-reg-shl-mask-fold

Found during a focused sweep after adding masked register-count shift
generation and after suppressing the known wide-int / mul-wide families.

The original saved fuzzer program was:

```text
/tmp/fuzzx-ptx-regshift-predalu-postm049-2H7kaYyJ/div-1779053516-18b077b11eb2dcd9
```

The first reducer output was invalid because it removed an `and.b32 %r33, ...,
31` mask while leaving a `shl.b32` that read `%r33`. The reducer now protects
the generator's high scratch register to avoid undefined shift-count
reductions. The valid reducer output was 287 lines; a straight-line live-value
pass reduced the checked-in PTX to 60 lines.

## Scalar Trace

For each thread:

```text
input = in[tid]
a = 52957 << (input & 31)
b = tid << 2
c = a + tid + b
d = 28155 << (c & 31)
e = c << (d & 31)
out = 65535 + e + ((28805 << 3) << 28 << 17)
```

The final `((28805 << 3) << 28 << 17)` term is zero modulo 32 bits; the
`bfe.s32` subchain reduces to shift amount 2; the interesting part is the
cascade of masked register-count `shl.b32` operations. `ptxas -O0` follows the
source trace. Optimized ptxas produces different values for 31 of the 32
lanes. Lane 0 happens to match because the thread-dependent shift inputs
collapse to zero in this shape.

For example, lane 1 stores `0x0034b744` at `-O0` and `0xd040ffff` at `-O2`.

This is a new register-shift family rather than the previous immediate-shift
or wide-int families: the reduced testcase has no body `mul.wide`, 64-bit
scratch ALU, `xor`, `or`, `lop3`, `selp`, video instruction, or predicated ALU
operation. It does require the masked register-count `shl.b32` chains added by
the new generator feature.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source with
`nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and compare the
printed output.

This reproduced on 2026-05-17 with CUDA Toolkit 13.2.1 nvcc/ptxas:

```text
release 13.2, V13.2.78
cuda_13.2.r13.2/compiler.37668154_0
```

For continued fuzzing past this family, use `DIV_DISABLE_REG_SHIFTS=1`.
