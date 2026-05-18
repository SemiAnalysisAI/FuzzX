# m039: sign-extended i8 high-byte pack loses sign bits at O2

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373 applied,
after adding pure-IR vector-reduction coverage. The original candidate was an
oracle finding: `-O0` matched the interpreter, while `-O2` dropped the bytes
that come from sign-extending an `i8` value.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m039-sext-i8-highbyte-pack/reduced.ll
```

Observed on the patched LLVM HEAD build:

```text
[1] input=0x00000001 O0=0x9eff79ff O2=0x9e007900 mismatch=true
```

## Reduction

For workitem `1`, `%salt == 0x9e3779b9`. The reduced program squares the low
seven bits:

```llvm
%byte = and i32 %salt, 127        ; 0x39
%mul = mul i32 %byte, %byte       ; 0x0cb1
%i8 = trunc i32 %mul to i8        ; 0xb1
%sext = sext i8 %i8 to i32        ; 0xffffffb1
```

The result packs bytes from `%salt` and from `%sext`. The low selected byte and
middle selected byte from `%sext` should both be `0xff`, so the expected result
is `0x9eff79ff`.

## Root Cause Notes

The `-O2` result is `0x9e007900`, which keeps the bytes taken directly from
`%salt` but clears the byte lanes derived from the sign-extended `i8`. This
points at an AMDGPU `-O2` combine/lowering bug for byte extraction or packing
after an `i8` sign-extension.

The reproducer is defined for the listed inputs: workitems are guarded by
`%wi < %n`, all shifts are constant and in range, and the arithmetic intentionally
uses wrapping LLVM integer semantics without `nuw`, `nsw`, `exact`, poison, or
`undef`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x9eff79ff`, `O2=0x9e007900`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373 applied | Reproduces: `O0=0x9eff79ff`, `O2=0x9e007900`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces: `O0=0x9eff79ff`, `O2=0x9e007900`. |

Original fuzzer input SHA-1:

```text
1f0b47f9efc701e5f099592949774d53a731a709
```

## Fuzzer Follow-Up

The fuzzer now suppresses `sext i8 to i32` values that feed high-byte extraction
by default. Set `FUZZX_ALLOW_M039_SEXT_I8_HIGHBYTE=1` to re-enable this shape.
