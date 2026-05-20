# m072: `((b & (a&c)) | (a&c)) & ~(a&c)` lowered to bitop3 `0x22` instead of `0x00`

The four-op chain reduces to a constant zero -- `(X & T) | T = T` because
`(X & T)` is a subset of `T`, and then `T & ~T = 0`.  At `-O0` on the HEAD
toolchains, the AMDGPU instruction selector folds the chain into a single
`v_bitop3_b32` and picks truth table `0x22` (which evaluates to `c & ~a`)
instead of the correct `0x00`.  Sibling shape to m071 but a distinct truth
table and a HEAD-only regression -- ROCm 7.2.3 produces zero for this
expression at `-O0`.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m072-bitop3-tand-or-and-not-zero/reduced.ll
```

With the instrumented LLVM HEAD build at `amdgpu/build/llvm-fuzzer`:

```text
input=0x12345678
O0=0xcc89a887
O2=0x00000000
mismatch=true
```

All active lanes store the same value (the kernel loads `a, b, c` from a
fixed offset).

## Root Cause Notes

For the chosen inputs (`a=0x12345678, b=0xCAFEBABE, c=0xDEADBEEF`):

* `a & c               = 0x12241668`
* `(b & (a&c)) | (a&c) = 0x12241668`  (= `a & c`)
* `& ~(a&c)            = 0x00000000`

`-O2` evaluates this chain after InstCombine has folded it to `0`, so SDAG
never even materialises a non-constant result.

`-O0` keeps all five SSA ops and emits:

```asm
s_load_dword s4, s[6:7], 0x0   ; s4 = a
s_load_dword s0, s[6:7], 0x4   ; s0 = b
s_load_dword s1, s[6:7], 0x8   ; s1 = c
v_mov_b32_e32 v1, s4           ; v1 = a
v_mov_b32_e32 v2, s1           ; v2 = c
v_bitop3_b32  v2, s0, v1, v2 bitop3:0x22   ; A=b, B=a, C=c
```

Truth table `0x22 = 0b00100010` evaluates to `1` only for minterms `m1`
(`A=0, B=0, C=1`) and `m5` (`A=1, B=0, C=1`), i.e. `c & ~a`.  For the
chosen inputs `c & ~a = 0xCC89A887`, exactly matching the wrong `-O0`
output.  The correct truth table for this expression is `0x00`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Passes: O0=O2=`0x00000000`. |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0xcc89a887`, `O2=0x00000000`. |
| ROCm HEAD with the same five PR patches applied locally | Reproduces: `O0=0xcc89a887`, `O2=0x00000000`. |

## Family

A structural sweep over `((X op1 T) op2 T) op3 ~T` shapes with `T` set to
`a op_t c` and `X` drawn from `{a, b, c}` produces 54 distinct failing
configurations on LLVM HEAD.  m072 picks the simplest sub-case where the
correct result is a constant zero; the rest are different shapes that all
reduce to a wrong `v_bitop3_b32` truth table.  Fixing this family probably
requires the SDAG bitop3 matcher to detect cases where a sub-expression and
its complement both appear in the same chain, or to evaluate the candidate
truth table against the actual boolean function rather than relying on
operand-position pattern matching.

## Fuzzer Suppression

Not yet suppressed -- this shape was found by direct code search outside the
oracle fuzzer's emit set.  Sibling family to m071 -- a single
`emitRandomBitop3StressIdiom` covering both shapes would be a good follow-up
if you want the fuzzer to drive reduction of the surrounding family.  No
`FUZZX_ALLOW_M072_*` flag is wired up because the fuzzer never generates
this exact shape today.
