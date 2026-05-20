# m070: scalar `fshl` by eight returns `x >> 24` at `-O0`

Found by code inspection while looking for new miscompiles in the AMDGPU
backend, then confirmed by direct execution.  The existing m015 and m016
reproducers picked shift counts (0 and 1) whose buggy outputs (`0` and "only
bit 31") were the easiest to recognise, which made it look like the bug class
was specific to those two shifts.  It is not.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m070-scalar-fshl-shift8/reduced.ll
```

With the instrumented LLVM build at `amdgpu/build/llvm-fuzzer`, the reduced
testcase produces:

```text
input=0xaabbccdd
O0=0x000000aa
O2=0xbbccdd00
mismatch=true
```

ROCm 7.2.3 does not reproduce this mismatch; both optimization levels return
`0xbbccdd00` (= `x << 8`).  ROCm HEAD reproduces with the same wrong O0
value.

## Root Cause Notes

The defined result of `fshl.i32(0xAABBCCDD, 0, 8)` is
`(0xAABBCCDD << 8) | (0 >> 24) = 0xBBCCDD00`.

At `-O0`, AMDGPU scalarizes the left input through `v_readfirstlane_b32` and
lowers the constant-shift `fshl` to a 64-bit logical right shift whose shift
amount is the *negation* of the constant shift count:

```asm
v_readfirstlane_b32 s4, v2     ; s4 = x
s_mov_b32           s0, 0      ; s0 = y = 0
s_mov_b32           s1, s4     ; [s0:s1] = (x in high, 0 in low) = x << 32
s_mov_b32           s4, -8     ; shift count = -8  (becomes 56 mod 64)
s_lshr_b64          s[0:1], s[0:1], s4
```

`(x << 32) >> 56 = x >> 24`, so the kernel stores `0xAA`.  The same pattern
holds for every constant shift `c >= 1`: the generated `s_lshr_b64` uses `-c`
as the shift amount (which is `64 - c` mod 64), and the result reduces to
`x >> (32 - c)` instead of `(x << c) | (y >> (32 - c))`.  Spot checks for
`c = 2,3,4,7,16,17,24,30,31` all reproduce, with the wrong O0 value matching
`x >> (32 - c)` in every case.  m015 (`c = 0`) takes a different, two-shift
path that ends with `s_lshr_b64 ..., -1` and also returns the wrong value.

`fshr.i32(x, y, c)` is unaffected -- `fshr.i32(0xAABBCCDD, 0, 8)` returns
`0xDD000000` at both optimization levels.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Passes: O0=O2=`0xbbccdd00`. |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0x000000aa`, `O2=0xbbccdd00`. |
| ROCm HEAD with the same five PR patches applied locally | Reproduces: `O0=0x000000aa`, `O2=0xbbccdd00`. |

## Fuzzer Suppression

Shares the `triggersM015M016ScalarFshl` suppressor at
`fuzzer/llvm_amdgpu_diff_fuzzer.cpp:953` with m015 and m016 -- that hook
already gates every scalar `llvm.fshl.i32` call regardless of shift count, so
this shape was already hidden behind the suppressor and never separately
reduced.  Set `FUZZX_ALLOW_M016_SCALAR_FSHL=1` to re-enable scalar
`llvm.fshl.i32` generation; no new flag is needed for m070.
