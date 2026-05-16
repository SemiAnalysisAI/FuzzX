# m006-ifconvert-not-xor

Found by continuing expanded structured-control-flow fuzzing with explicit
`lop3.b32`, `min/max`, `mul.hi`, and `prmt.b32` generation disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1 DIV_DISABLE_MINMAX=1 \
DIV_DISABLE_MULHI=1 DIV_DISABLE_PRMT=1 DIV_MIN_BLOCKS=4 \
DIV_MAX_BLOCKS=20 DIV_MAX_INSTS_PER_BLOCK=10 DIV_WORKING_REGS=12 \
DIV_MAX_LOOP_ITERS=32 DIV_MAX_IMMEDIATE=1024
seed 0x18af810ebb3cdf94
```

The original saved fuzzer program is in
`/tmp/fuzzx-structured-expanded-nolop3-nominmax-nomulhi-noprmt/div-1778782557-18af810ebb3cdf94`
on the machine where this was reduced. The minimized PTX in `reduced.ptx` no
longer reads input memory and launches five threads.

## Correct scalar trace

The standalone launch passes `n = 32` and launches tids 0..4.

For tids other than 4, the branch skips the else path and leaves `%r0 = n`:

```text
out = 594 * n + n = 594 * 32 + 32 = 0x00004a60
```

For tid 4, the else path runs:

```text
%r6 = ~tid = ~4 = 0xfffffffb
%r0 = n ^ %r6 = 32 ^ 0xfffffffb = 0xffffffdb
out = 594 * 32 + 0xffffffdb = 0x00004a1b
```

`ptxas -O0` matches those values. `ptxas -O2` and `-O3` store `0x00004a64`
for tid 4, as if the else path computed `n ^ tid` instead of `n ^ ~tid`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

The latest checked CUDA Toolkit ptxas on 2026-05-14 was CUDA 13.2.1
(`cuda-nvcc-13-2_13.2.78-1_arm64.deb`). SASS below was decoded with matching
CUDA 13.2.1 `nvdisasm` V13.2.78, build `cuda_13.2.r13.2/compiler.37668154_0`.

## SASS root cause

At `-O3`, ptxas if-converts the branch and folds `not` + `xor` into a
predicated `LOP3.LUT`:

```text
S2R     R5, SR_TID.X ;
LDC     R7, c[0x0][0x388] ;        // n
ISETP.NE.U32.AND P0, PT, R5, 0x4, PT ;
@!P0    LOP3.LUT R0, R7, 0x4, RZ, 0x3c, !PT ;
IMAD    R5, R7, 0x252, R0 ;
```

For the false path, ptxas has proved `tid == 4` and substituted the literal
`0x4`, which is fine. The bug is that the complement from `not.b32` is lost:
truth table `0x3c` computes `n ^ 4`, not `n ^ ~4`. That gives:

```text
594 * 32 + (32 ^ 4) = 0x00004a64
```

The correct false-path value is:

```text
594 * 32 + (32 ^ ~4) = 0x00004a1b
```

This is not m001's loop predicate bug, m002's explicit `lop3.b32` bug, m003's
signed-max chain bug, m004's `mul.hi` loop trip-count bug, or m005's PRMT
if-conversion bug. It does show that disabling explicit `lop3.b32` is not
enough to avoid optimizer-generated LOP3 bugs; this case starts from
`not.b32` plus `xor.b32`.
