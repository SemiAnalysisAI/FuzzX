# m041: high-byte pack after arithmetic shift lowers to wrong `v_perm_b32`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, and llvm/llvm-project#198412 applied. The original
fuzzer program used loop-carried clamp/pack idioms; the minimized reproducer
keeps only the final byte-pack expression.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m041-ashr-highbyte-pack/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0x00000000
O0=0x0700ff07
O2=0x07bdff07
mismatch=true
```

The bad `O2` byte is not stable; repeated runs have also produced values such
as `0x073dff07`, `0x077dff07`, and `0x07fdff07`.

## Reduction

For the single-element reproducer, the runner passes `%n == 1` and
`%wi == 0`, so:

```llvm
%x = add i32 (shl i32 2047, 16), 1 ; 0x07ff0001
%ashr = ashr i32 %x, 8             ; 0x0007ff00
```

The expected packed bytes are:

```text
byte0 = (%ashr >> 16) & 0xff = 0x07
byte1 = (%x    >>  8) & 0xff = 0xff
byte2 = (%ashr >>  8) & 0xff = 0x00
byte3 = %x & 0xff000000       = 0x07
```

So the defined result is `0x0700ff07`.

## Root Cause Notes

At `-O2`, AMDGPU recognizes the byte pack and lowers it to one `v_perm_b32`:

```asm
v_add_u32_e32 v0, s4, v0
v_add_u32_e32 v0, 0x7ff0000, v0
v_perm_b32 v0, v0, v0, 0x07000607
```

That permutation does not preserve the zero high byte produced by the
arithmetic shift of the known-positive `%x`; the third output byte is wrong and
can vary across runs. `-O0` emits the shift, byte extracts, masks, and ors
directly and returns the expected value.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x0700ff07`; observed bad `O2` values include `0x073dff07`, `0x077dff07`, `0x07bdff07`, and `0x07fdff07`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Reproduces: `O0=0x0700ff07`; observed bad `O2` values include `0x077dff07`, `0x07bdff07`, and `0x07fdff07`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Reproduces: `O0=0x0700ff07`; observed bad `O2` values include `0x07bdff07`. |

## Fuzzer Follow-Up

The fuzzer now rejects high-byte extraction from an `ashr i32` value when it is
fed into byte-pack shapes. Set `FUZZX_ALLOW_M041_ASHR_HIGHBYTE_PACK=1` to
re-enable this bug class.
