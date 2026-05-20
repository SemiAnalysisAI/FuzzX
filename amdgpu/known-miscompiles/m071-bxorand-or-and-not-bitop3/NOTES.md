# m071: `((b ^ (c & a)) | (c & a)) & ~(c & a)` lowered with wrong bitop3 truth table at `-O0`

Found by code inspection (specifically by enumerating three-operand boolean
expressions where the same subexpression `t = c & a` appears multiple times in
both the LHS and RHS of an `and` -- the same code-smell that produced the
m020--m029 family of bitop3 truth-table bugs), then confirmed by direct
execution on gfx950.  Sibling bug class to m020, m023, m027 but a distinct
shape that PR 198556 does not catch.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m071-bxorand-or-and-not-bitop3/reduced.ll
```

With any of the three campaign toolchains (LLVM HEAD with the five PR
patches, ROCm 7.2.3 source build, ROCm HEAD with the same patches) the
reduced testcase produces:

```text
input=0x12345678
O0=0xc8dae896
O2=0xc8daa896
mismatch=true
```

Every active lane stores the same value because the kernel loads `a, b, c`
from a fixed offset (`%in[0..2]`) -- so the mismatch reproduces on every lane,
not just lane 0.

## Root Cause Notes

The defined result is `b & ~(c & a)`: applying the identity `(X ^ Y) | Y =
X | Y` to `(b ^ t) | t` reduces the expression to `(b | t) & ~t`, which
further reduces to `b & ~t`.  For the chosen inputs,
`b & ~(c & a) = 0xCAFEBABE & ~(0xDEADBEEF & 0x12345678) = 0xC8DAA896`.

At `-O0` the AMDGPU instruction selector folds the four-op tail into a single
`v_bitop3_b32` and picks truth table `0x72`, which is wrong by exactly one
minterm:

```asm
; relevant fragment of the -O0 lowering
s_load_dword s1, s[4:5], 0x0   ; s1 = a
s_load_dword s0, s[4:5], 0x4   ; s0 = b
s_load_dword s4, s[4:5], 0x8   ; s4 = c
v_mov_b32_e32 v2, s4           ; v2 = c
v_mov_b32_e32 v3, s1           ; v3 = a
v_bitop3_b32  v2, s0, v2, v3 bitop3:0x72   ; A=b, B=c, C=a
```

The truth table for the correct function `r(a,b,c) = b & ~(c & a)` indexed
by `(A=b, B=c, C=a)` is `0x70` (minterms `m4`, `m5`, `m6`).  The selector
emits `0x72`, which additionally lights `m1 = (A=0, B=0, C=1) = (b=0, c=0,
a=1)` -- that is exactly the extra bit observed in the output (byte 1, bit 6
of `0xC8DAE896` vs `0xC8DAA896`).

`-O2` evaluates the same SSA chain after InstCombine has folded it to a
plain `b & ~(c & a)` (two ops), so SDAG only sees a single `s_andn2_b32` /
`s_and_b32` pair and the bitop3 matcher never fires.

Spot checks with the operands re-associated (`t = a & c` instead of
`c & a`) flip which lanes reproduce but the bug remains; the truth-table
selection appears to be operand-order-sensitive.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Reproduces: `O0=0xc8dae896`, `O2=0xc8daa896`. |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0xc8dae896`, `O2=0xc8daa896`. |
| ROCm HEAD with the same five PR patches applied locally | Reproduces: `O0=0xc8dae896`, `O2=0xc8daa896`. |

## Fuzzer Suppression

Not yet suppressed -- this shape was found by direct code search outside the
oracle fuzzer's emit set.  Add an `emitRandom*` for this idiom (or a generic
"three-operand bitop3 stress" emitter) if you want the fuzzer to drive
reduction of the surrounding family.  No `FUZZX_ALLOW_M071_*` flag is wired
up because the fuzzer never generates this exact shape today.
