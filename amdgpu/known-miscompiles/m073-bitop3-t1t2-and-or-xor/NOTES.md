# m073: `((a&b) & (a|c)) ^ ((a&b) | (a|c))` lowered to bitop3 `0x5e` instead of `0x1e`

Found by sweeping five-operand expressions where two distinct subexpressions
each appear twice in the result chain.  Structurally different from m071 and
m072 -- there is no `~T` complement here; instead the bitop3 selector takes
the AND and OR of two earlier intermediate values and gets exactly one
minterm wrong.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m073-bitop3-t1t2-and-or-xor/reduced.ll
```

With any of the three campaign toolchains:

```text
input=0x12345678
O0=0xdc99ecd7
O2=0xdc89ecc7
mismatch=true
```

All active lanes store the same value because `a, b, c` are loaded from a
fixed offset.

## Root Cause Notes

The expression `((a&b) & (a|c)) ^ ((a&b) | (a|c))` reduces to `(a&b) ^ (a|c)`
because `(X & Y) ^ (X | Y) = X ^ Y` for any X, Y.

For the chosen inputs `(a&b) ^ (a|c) = 0x02341238 ^ 0xDEBDFEFF = 0xDC89ECC7`,
which is the correct `-O2` output.

At `-O0` the AMDGPU instruction selector folds the four bitwise ops after the
two ANDs into a single `v_bitop3_b32`.  It emits truth table `0x5e`:

```asm
s_load_dword s4, s[6:7], 0x0       ; s4 = a
s_load_dword s0, s[6:7], 0x4       ; s0 = b
s_load_dword s1, s[6:7], 0x8       ; s1 = c
s_and_b32     s0, s4, s0           ; s0 = a & b
v_mov_b32_e32 v1, s4               ; v1 = a
v_mov_b32_e32 v2, s1               ; v2 = c
v_bitop3_b32  v2, s0, v1, v2 bitop3:0x5e   ; A=a&b, B=a, C=c
```

With inputs `(A=a&b, B=a, C=c)` the correct boolean function is
`A ^ (B | C)` -- truth table `0x1e` (= `0b00011110`, minterms `m1..m4`).
The selector emits `0x5e` (= `0b01011110`), which additionally lights `m6
= (A=1, B=1, C=0)`.  That extra minterm fires at exactly the bit positions
where `a=1, b=1, c=0`; for the chosen inputs that is bit 4 of byte 0 and
bit 4 of byte 2, giving the observed differences `0xC7 → 0xD7` and
`0x89 → 0x99`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Reproduces: `O0=0xdc99ecd7`, `O2=0xdc89ecc7`. |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0xdc99ecd7`, `O2=0xdc89ecc7`. |
| ROCm HEAD with the same five PR patches applied locally | Reproduces: `O0=0xdc99ecd7`, `O2=0xdc89ecc7`. |

## Family

m071, m072, m073 are three distinct shapes that all expose `v_bitop3_b32`
truth-table miscompiles at `-O0`:

| Bug | Shape | Correct table | Generated table | Notes |
| --- | --- | --- | --- | --- |
| m071 | `((b^T)|T) & ~T`, `T = c & a` | `0x70` | `0x72` | one extra minterm; ROCm 7.2.3 also affected |
| m072 | `((b&T)|T) & ~T`, `T = a & c` (trivially zero) | `0x00` | `0x22` | two extra minterms; HEAD-only |
| m073 | `((a&b) & (a|c)) ^ ((a&b) | (a|c))` | `0x1e` | `0x5e` | one extra minterm; all three toolchains |

All three look like the AMDGPU bitop3 selector evaluating its candidate
truth table against an incorrect minterm-by-minterm model rather than
checking the actual boolean function.

## Fuzzer Suppression

Not yet suppressed -- this shape was found by direct code search outside the
oracle fuzzer's emit set.  An `emitRandomBitop3StressIdiom` that walks the
5-op space (two intermediate values combined via AND/OR/XOR) would cover
both m073 and the m071/m072 family if you want the fuzzer to drive
reduction of the surrounding bugs.  No `FUZZX_ALLOW_M073_*` flag is wired
up because the fuzzer never generates this exact shape today.
