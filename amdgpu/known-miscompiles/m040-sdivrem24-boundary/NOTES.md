# m040: signed DIVREM24 quotient is one too large

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373 and
llvm/llvm-project#196418 applied. The original fuzzer program amplified the
bad quotient through an `i16` byte swap and vector reduction; the minimized
reproducer stores the signed division result directly.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m040-sdivrem24-boundary/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[13] input=0x000000f0 O0=0x0000df10 O2=0x0000df11 mismatch=true
```

## Reduction

For work-item 13, the reduced IR computes:

```llvm
%num = add i32 %wi, 13762291
%den.mask = and i32 %v, 255
%den = or i32 %den.mask, 1
%q = sdiv i32 %num, %den
```

With `%wi == 13` and `%v == 0x000000f0`, `%num` is `0x00d1ff00`
and `%den` is `0x000000f1`. The signed quotient is `0x0000df10`
with remainder `0x000000f0`.

## Root Cause Notes

At `-O2`, AMDGPU lowers this `sdiv` through the 24-bit float reciprocal
division path:

```asm
v_cvt_f32_u32_e32
v_rcp_f32_e32
v_mul_f32_e32
v_trunc_f32_e32
v_cvt_u32_f32_e32
```

That path is only valid when the signed operands fit the signed 24-bit range.
The numerator `0x00d1ff00` has bit 23 set, so it does not fit as a positive
signed 24-bit value. The reciprocal estimate/correction sequence returns
`0x0000df11`, one greater than the defined quotient. `-O0` uses the precise
lowering and returns `0x0000df10`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x0000df10`, `O2=0x0000df11`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373 and llvm/llvm-project#196418 applied locally | Reproduces: `O0=0x0000df10`, `O2=0x0000df11`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373 and llvm/llvm-project#196418 applied locally | Reproduces: `O0=0x0000df10`, `O2=0x0000df11`. |

## Fuzzer Follow-Up

The fuzzer now masks generated signed `sdiv` / `srem` numerators to the
positive signed 24-bit range before using the small odd denominator idiom.
Validation also rejects unmasked signed div/rem by small odd denominators by
default. Set `FUZZX_ALLOW_M040_SIGNED_DIVREM24=1` to re-enable this bug class.
