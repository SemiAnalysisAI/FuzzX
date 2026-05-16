# m002: `selp`/`lop3` fold miscompile

Found while running the structured-control-flow generator added after m001.
This is not the same failure mode as m001: the reduced testcase has no loop,
no branch, and no uniform predicate/register control flow.  A plain label that
splits the block before the `setp`/`selp` sequence is enough to expose it.

## Reduced PTX Behavior

The reduced kernel takes only an output pointer and stores one `u32` per
thread at `out[tid]`.  All threads compute the same scalar value:

```ptx
mov.u32       %r3, 1;
mov.u32       %r7, 0;
label:
setp.lt.s32   %p1, %r3, 15;        // true
selp.b32      %r0, %r3, %r7, %p1;  // %r0 = 1
lop3.b32      %r7, 26, %r0, %r0, 0x65;
xor.b32       %r0, %r7, 26;        // %r0 = 0xfffffffe
```

Observed output:

* `-O0`: every thread stores `0xfffffffe` (correct).
* `-O1`, `-O2`, `-O3`: every thread stores `0xffffffff` (wrong).

The same reduced testcase reproduces with CUDA 13.0 ptxas V13.0.88 and CUDA
13.2 Update 1 ptxas V13.2.78 for `sm_103`.

## SASS

CUDA 13.0 `-O0` keeps the `selp`, `lop3`, and `xor` as separate operations:

```sass
MOV R2, 0x1 ;
MOV R3, RZ ;
ISETP.LT.AND P0, PT, R2, 0xf, PT ;
SEL R3, R2, R3, P0 ;
LOP3.LUT R4, R3, 0x1a, R3, 0x59, !PT ;
LOP3.LUT R9, R4, 0x1a, RZ, 0x3c, !PT ;
STG.E desc[UR4][R2.64], R9 ;
```

CUDA 13.0 and CUDA 13.2.1 `-O2` fold the value computation to:

```sass
HFMA2 R0, -RZ, RZ, 0, 5.9604644775390625e-08 ;
LOP3.LUT R5, R0, 0x1a, RZ, 0x95, !PT ;
STG.E desc[UR4][R2.64], R5 ;
```

That folded `LOP3.LUT` computes `0xffffffff` for the selected value `1`; the
unfolded PTX sequence computes `0xfffffffe`.  This points at an incorrect
`selp`/`lop3`/`xor` boolean fold or truth-table rewrite across a basic-block
boundary, not at uniform loop predicate analysis.
