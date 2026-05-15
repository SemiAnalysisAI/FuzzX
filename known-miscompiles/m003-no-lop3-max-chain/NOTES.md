# m003-no-lop3-max-chain

Found from the structured-control-flow fuzzer run with explicit `lop3.b32`
generation disabled:

```text
DIV_STRUCTURED_CONTROL_FLOW=1 DIV_DISABLE_LOP3=1
seed 0x18af79fcbae68d75
```

The original saved fuzzer program is in
`/tmp/fuzzx-structured-nolop3/div-1778774636-18af79fcbae68d75` on the
machine where this was reduced. The minimized PTX in `reduced.ptx` no longer
has control flow or input memory; it is a straight-line `sub`/`max.s32` chain.

## Correct scalar trace

The standalone launch used for reduction passes `n = 32` and launches one
thread. PTX scalar semantics:

```text
%r0 = n = 32
%r1 = 0
%r2 = %r0 - %r0 = 0
%r1 = max.s32(0, 0) = 0
%r2 = 0 - 32 = -32
%r1 = max.s32(-32, 0) = 0
%r2 = -32 - 32 = -64
%r1 = max.s32(-64, 0) = 0
%r2 = -64 - 32 = -96
%r1 = max.s32(-96, 0) = 0
store %r1 = 0
```

So the correct output is `0x00000000`. `ptxas -O0` stores `0x00000000`.
`ptxas -O1`, `-O2`, and `-O3` store `0x00000020`.

CUDA inline-PTX repro: `repro_nvcc_inline_ptx.cu`. Build the same source
with `nvcc -Xptxas -O0` and `nvcc -Xptxas -O2`, run both binaries, and
compare the printed output.

This reproduced on 2026-05-14 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
* CUDA Toolkit 13.2.1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`

## SASS root cause

At `-O0`, ptxas emits the expected recurrence. The relevant SASS is:

```text
IADD3   R3, PT, PT, R0, -R0, RZ ;      // %r2 = n - n = 0
VIMNMX  R2, PT, PT, R3, R2, !PT ;      // max(0, 0)
IADD3   R3, PT, PT, R3, -R0, RZ ;      // -n
VIMNMX  R2, PT, PT, R3, R2, !PT ;
IADD3   R3, PT, PT, R3, -R0, RZ ;      // -2n
VIMNMX  R2, PT, PT, R3, R2, !PT ;
IADD3   R0, PT, PT, R3, -R0, RZ ;      // -3n
VIMNMX  R0, PT, PT, R0, R2, !PT ;
STG.E   ..., R0 ;
```

At `-O3`, ptxas combines the chain into `VIADDMNMX`/`VIMNMX3`:

```text
LDC       R4, c[0x0][0x388] ;          // n
IMAD.U32  R5, R4.reuse, -0x2, RZ ;     // -2n
VIADDMNMX R0, R4, -R4, RZ, !PT ;       // max(n - n, 0) = 0
VIMNMX3   R0, R0, R4, R5, !PT ;        // max(0, n, -2n) = n   <-- wrong term
VIADDMNMX R5, R5, -R4, R0, !PT ;       // max(-3n, n) = n
STG.E     ..., R5 ;
```

The optimized code includes `+n` as one of the max candidates. The source chain
never takes `max` over the pre-subtract value `n`; the candidates are
`0, -n, -2n, -3n`. For positive `n`, the correct signed maximum is `0`, but the
optimized SASS computes `n`.

This is therefore not the earlier m001 loop/uniform-register bug and not the
m002 `lop3.b32` fold bug. The minimized testcase is straight-line code and has
no explicit `lop3.b32`.
