# m060: pack/unpack byte dot is miscompiled as `v_dot4_u32_u8`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0x1F98
o2=0x1E35
expected=0x1F98
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m060-packunpack-bytedot-dot4/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x00000000 O2=0x00000000 mismatch=false
[1] input=0x00000001 O0=0x00001f98 O2=0x00001e35 mismatch=true
any_mismatch=true
```

## Reduction

The reduced kernel keeps the generated `packunpack` byte-dot tail. For lane 1,
the final scalar IR computes three byte products and adds them:

```llvm
%fuzz.packunpack.idiom.byte.mul0 = mul i32 %fuzz.packunpack.idiom.a0.zext, %fuzz.packunpack.idiom.b0.zext
%fuzz.packunpack.idiom.byte.mul1 = mul i32 %fuzz.packunpack.idiom.a1.zext, %fuzz.packunpack.idiom.b1.zext
%fuzz.packunpack.idiom.byte.mul2 = mul i32 %fuzz.packunpack.idiom.a2.zext, %fuzz.packunpack.idiom.b2.zext
%fuzz.packunpack.idiom.byte.sum01 = add i32 %fuzz.packunpack.idiom.byte.mul0, %fuzz.packunpack.idiom.byte.mul1
%fuzz.packunpack.idiom.byte.sum = add i32 %fuzz.packunpack.idiom.byte.sum01, %fuzz.packunpack.idiom.byte.mul2
```

The LLVM interpreter in the original fuzzer finding and the `-O0` GPU compile
agree on `0x00001f98`. The `-O2` GPU compile returns `0x00001e35`.

## Root Cause Notes

The reduced `-O0` assembly computes the three products directly near the final
store:

```asm
v_mul_lo_u32 v2, v2, v7
v_mul_lo_u32 v3, v3, v6
v_mul_lo_u32 v4, v4, v5
v_add3_u32 v2, v2, v3, v4
global_store_dword v[0:1], v2, off
```

The `-O2` lowering recognizes the byte-dot-like expression and rewrites it into
a packed byte dot:

```asm
v_perm_b32 v3, v3, v7, s10
v_mul_u32_u24_sdwa v4, v2, v4 dst_sel:DWORD dst_unused:UNUSED_PAD src0_sel:BYTE_3 src1_sel:BYTE_1
v_perm_b32 v2, v2, v2, s11
v_dot4_u32_u8 v2, v2, v3, v4
global_store_dword v[0:1], v2, off
```

That packed form does not match the scalar three-product expression for lane 1.
This looks like a `v_dot4_u32_u8` combine/lowering bug, likely in the byte
packing or accumulator operand chosen for the synthesized fourth lane.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: lane 1 `O0=0x00001f98`, `O2=0x00001e35`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x00001f98`, `O2=0x00001e35`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x00001f98`, `O2=0x00001e35`. |

Original fuzzer input SHA-1:

```text
8927f8c07ad88af3bd16db9b0deb3d101edfa222
```

## Fuzzer Follow-Up

The fuzzer now rejects final stores depending on generated `packunpack`
byte-dot sums by default. Set `FUZZX_ALLOW_M060_PACKUNPACK_BYTEDOT=1` to
re-enable this bug class.
