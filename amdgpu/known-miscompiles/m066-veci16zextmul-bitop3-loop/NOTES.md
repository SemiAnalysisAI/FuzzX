# m066: `<4 x i16>` zext-mul reduce inside a loop, feeding a bitop3 cascade, is miscompiled at `-O2`

Found while fuzzing LLVM HEAD with llvm/llvm-project#196418,
llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508,
and llvm/llvm-project#198556 applied.  The original oracle finding was:

```text
kind=oracle
index=0
input=0x0
o0=0x2BE83DE2
o2=0x8BD601F1
expected=0x2BE83DE2
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m066-veci16zextmul-bitop3-loop/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches, on ROCm HEAD, and on
ROCm 7.2.3 (all three reproduce — this is a long-standing bug, not a HEAD
regression):

```text
input=0x00000000
O0=0x2be83de2
O2=0x8bd601f1
mismatch=true
```

In this case `-O0` matches the LLVM-interpreter oracle (`0x2BE83DE2`); `-O2`
produces `0x8BD601F1` instead.

## Reduction

`llvm-reduce` reduced the 219-line generated IR to 86 lines.  The reduced
kernel keeps a 12-iteration counted loop whose body builds a `<4 x i16>`
from the lo and hi halves of the accumulator, zext-multiplies it against a
`<4 x i16>` of constants and a half-bit-rotated copy, xors the result with
its own shuffle, extracts two lanes, smaxes them, and xors the smax back
into the accumulator.  The loop-exit value is then run through an
`umaxbitop3cascade`-shaped sequence of `and`/`xor`/`or`/`add` ops with
constants before being stored.

```llvm
fuzz.loop.body:
  %fuzz.cfg.veci16zextmul.idiom.half.trunc = trunc i32 %fuzz.loop.acc to i16
  %fuzz.cfg.veci16zextmul.idiom.half.shr = lshr i32 %fuzz.loop.acc, 16
  %fuzz.cfg.veci16zextmul.idiom.half.trunc5 = trunc nuw i32 ... to i16
  %0 = insertelement <4 x i16> <i16 poison, i16 -21013, i16 poison, i16 -31491>,
                                i16 %...half.trunc, i64 0
  %fuzz.vec.ins14 = insertelement <4 x i16> %0, i16 %...half.trunc5, i64 2
  %fuzz.cfg.veci16zextmul.idiom.other.trunc22 = or i16 %...half.trunc5, 1
  %fuzz.vec.ins30 = insertelement <4 x i16> <i16 -21013, ..., poison, poison>,
                                  i16 %...other.trunc22, i64 2
  %fuzz.vec.ins31 = insertelement <4 x i16> %fuzz.vec.ins30,
                                  i16 %...other.trunc22, i64 3
  %fuzz.cfg.veci16zextmul.idiom.v.zext = zext <4 x i16> %fuzz.vec.ins14 to <4 x i32>
  %fuzz.cfg.veci16zextmul.idiom.w.zext = zext <4 x i16> %fuzz.vec.ins31 to <4 x i32>
  %fuzz.cfg.veci16zextmul.idiom.prod = mul nuw <4 x i32> %v.zext, %w.zext
  %fuzz.cfg.veci16zextmul.idiom.prod.rev =
      shufflevector <4 x i32> %prod, <4 x i32> zeroinitializer,
                    <4 x i32> <i32 poison, i32 2, i32 poison, i32 0>
  %fuzz.cfg.veci16zextmul.idiom.sum.rev = xor <4 x i32> %prod, %prod.rev
  %lane32 = extractelement <4 x i32> %sum.rev, i64 1
  %lane35 = extractelement <4 x i32> %sum.rev, i64 3
  %fuzz.cfg.veci16zextmul.idiom.reduce.sminmax.smax37 =
      call i32 @llvm.smax.i32(i32 %lane32, i32 %lane35)
  %fuzz.cfg.veci16zextmul.idiom.a.xor = xor i32 %...smax37, %fuzz.loop.acc
```

## Root Cause Notes

The bug appears to be in the `-O2` lowering of the loop body's `<4 x i32>
mul` + shuffle + xor + extract + `llvm.smax.i32` sequence — most likely
loop-unrolling combined with vector-mul folding that drops or reorders one
of the constant lanes.  `-O0` keeps the vector ops scalarized in straight
line per iteration and matches the LLVM-interpreter oracle.  Because the
ROCm 7.2.3 release reproduces too, the bug pre-dates the HEAD bitop3 /
SDWA fixes the current campaign is built against.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x2be83de2`, `O2=0x8bd601f1`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#196418, llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508, llvm/llvm-project#198556 applied locally | Reproduces: `O0=0x2be83de2`, `O2=0x8bd601f1`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with the same five PR patches applied locally | Reproduces: `O0=0x2be83de2`, `O2=0x8bd601f1`. |

Original fuzzer input SHA-1:

```text
0131b7660911c4dd440c0c500621e73650f09538
```

Reduced reproducer SHA-1:

```text
13b2f179bf8354c358f184da7ddf3245dca50fde
```

## Fuzzer Follow-Up

The fuzzer now rejects loop-carried final stores that depend on both
`fuzz.veci16zextmul.idiom` values and the i32 loop accumulator by default.
Set `FUZZX_ALLOW_M066_VECI16ZEXTMUL_BITOP3_LOOP=1` to re-enable this bug
class.
