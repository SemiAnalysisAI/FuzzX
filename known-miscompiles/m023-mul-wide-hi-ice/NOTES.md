# m023-mul-wide-hi-ice

Found while re-enabling straight-line `mul.hi.{u32,s32}` generation with loops
and earlier known bug classes disabled:

```text
DIV_MIN_BLOCKS=1
DIV_MAX_BLOCKS=1
DIV_MIN_INSTS_PER_BLOCK=24
DIV_MAX_INSTS_PER_BLOCK=48
DIV_DISABLE_ARBITRARY_LOOPS=1
DIV_DISABLE_STRUCTURED_LOOPS=1
DIV_DISABLE_LOP3=1
DIV_DISABLE_MINMAX=1
DIV_DISABLE_PRMT=1
DIV_DISABLE_NOT=1
DIV_DISABLE_CNOT=1
DIV_DISABLE_ABS=1
DIV_DISABLE_SIGNED_CMP=1
DIV_DISABLE_FUNNEL=1
DIV_DISABLE_NEG=1
DIV_DISABLE_SIGNED_SHR=1
DIV_DISABLE_BFIND=1
DIV_DISABLE_BMSK=1
DIV_DISABLE_ADDC=1
DIV_DISABLE_SUBC=1
DIV_DISABLE_SET=1
DIV_DISABLE_S32_SLCT=1
DIV_DISABLE_VSUB4=1
seed 0x18afae60699908be
```

The original saved fuzzer program was:

```text
/tmp/ptx-fuzz-straightline-mulhi-known-disabled-100k/div-1778832149-18afae60699908be
```

The minimized PTX in `reduced.ptx` is straight-line and has no input buffer.
It launches one thread, receives `n = 32`, and stores one u32.

## Correct scalar trace

```text
%r0 = n = 32
%r1 = tid = 0
%rd2 = mul.wide.s32(%r0, 0xffffffff)
     = 32 * -1 = 0xffffffffffffffe0
%r4 = low32(%rd2) = 0xffffffe0
%r4 = mul.hi.s32(32, 0xffffffe0)
    = high32(32 * -32)
    = high32(0xfffffffffffffc00)
    = 0xffffffff
%p0 = setp.ge.u32(%r4, %r1) = true
%r7 = 0xffffffa5
store %r7
```

So the correct output for a one-thread launch is `0xffffffa5`, and `ptxas -O0`
compiles and runs that value. `ptxas -O1`, `-O2`, and `-O3` do not compile the
kernel.

Standalone C++ bug-report repro:
`repro_ptxas_mul_wide_hi_ice_o2.cpp`. It embeds the reduced PTX, compiles it
with `ptxas -O0`, launches the `-O0` cubin to check the scalar output, then
compiles with `ptxas -O2` and returns 1 when the optimized compiler crashes.

This reproduced on 2026-05-15 with both:

* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`
  (`ptxas` segfaults at `-O1`, `-O2`, and `-O3`)
* CUDA Toolkit 13.2 Update 1 ptxas, the latest NVIDIA CUDA Toolkit listed on
  NVIDIA's CUDA Toolkit Archive on 2026-05-15:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`
  (`ptxas fatal : (C7907) Internal compiler error`)

## Root cause notes

The reduced source needs the `mul.wide` low-half producer feeding
`mul.hi.s32`:

```text
mul.wide.s32 %rd2, %r0, 0xffffffff;
mov.b64      {%r4, %r5}, %rd2;
mul.hi.s32   %r4, %r0, %r4;
```

The ICE disappears if that producer is replaced with an equivalent low-32-bit
operation such as:

```text
sub.u32    %r4, 0, %r0;
```

or:

```text
mul.lo.u32 %r4, %r0, 0xffffffff;
```

It also disappears if `n` is replaced by a literal `32`. This points at an
optimizer crash in the value/range rewrite for a parameter-derived `mul.wide`
low half feeding a signed high multiply and then an unsigned predicate/select.
This is distinct from m004's `mul.hi` loop-trip-count wrong-code bug: this
test has no loop, and the optimized compiler crashes before producing SASS.
