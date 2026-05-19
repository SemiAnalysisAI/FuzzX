# m061: overflow-mask-pack chain is miscompiled at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=0
input=0x0
o0=0xA1DF8800
o2=0xA0DF8400
expected=0xA0DF8400
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m061-ovmaskpack-o0-overflow-lowering/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0x00000000
O0=0xa1df8800
O2=0xa0df8400
mismatch=true
```

## Reduction

The reduced kernel keeps the generated overflow-mask-pack tail. The final value
is built from `*.with.overflow` intrinsics, selected result bytes, packed byte
lanes, and a fold of the raw overflow intrinsic results:

```llvm
%fuzz.ovmaskpack.idiom.ov.call8 =
  call { i32, i1 } @llvm.ssub.with.overflow.i32(...)
%fuzz.ovmaskpack.idiom.ov.call56 =
  call { i32, i1 } @llvm.usub.with.overflow.i32(...)
%fuzz.ovmaskpack.idiom.fold.xor =
  xor i32 %fuzz.ovmaskpack.idiom.pack.add79, %fuzz.ovmaskpack.idiom.fold71
store i32 %fuzz.ovmaskpack.idiom.fold.xor, ptr addrspace(1) %out.ptr, align 4
```

`llvm-reduce` reduced the original 1109-line IR to 706 lines before instruction
reduction stopped making progress. A hand-isolated overflow tail with only a
constant seed did not reproduce, so the remaining producer chain still matters
for the `-O0` backend shape.

## Root Cause Notes

The reduced `-O0` assembly lowers the overflow checks through clamp/compare and
carry sequences near the final store, for example:

```asm
v_sub_i32 v2, s0, v5 clamp
v_sub_u32_e64 v8, s0, v5
v_cmp_ne_u32_e64 s[4:5], v8, v2
...
v_sub_co_u32_e64 v9, s[2:3], s0, v5
...
global_store_dword v[0:1], v2, off
```

That `-O0` lowering stores `0xa1df8800`. The LLVM interpreter and the `-O2` GPU
compile agree on `0xa0df8400`. The optimized IR compiled back at `-O0` also
returns the expected value, which points to an `-O0` lowering issue in the
unoptimized overflow/byte-pack shape rather than the optimized IR.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=0xa0df8400`, `O2=0xa0df8400`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0xa1df8800`, `O2=0xa0df8400`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0xa1df8800`, `O2=0xa0df8400`. |

Original fuzzer input SHA-1:

```text
1ab772e8aa980d21f1c2267720de6f6f641f63fb
```

Reduced reproducer SHA-1:

```text
231a333377d6fede3b2bcc9790d05bd32928d216
```

## Fuzzer Follow-Up

The fuzzer now rejects final stores depending on generated `ovmaskpack` values
by default. Set `FUZZX_ALLOW_M061_OVMASKPACK_OVERFLOW=1` to re-enable this bug
class.
