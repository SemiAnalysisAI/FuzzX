# m034: `fshl`/add chain folded to byte dot product with wrong zero case

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after adding overflow-intrinsic generation. The original fuzzer program
exposed the bug through a later FP16 vector subtract, but the reduced testcase
does not need FP or overflow intrinsics.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m034-fshl-add-workitem-product/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0xc0000000
O2=0xffffffff
mismatch=true
```

## Reduction

For the reproducer input and the single launched workitem, `%base == 0`,
`%idx == 0`, and therefore `%x == (%base & 1023) * (%idx & 1023) == 0`.
The remaining expression is:

```llvm
%neg = sub i32 -1, %x
%lhs = add i32 %neg, -2147483648
%fshl = call i32 @llvm.fshl.i32(i32 %lhs, i32 %x, i32 30)
%result = add i32 %fshl, %x
```

With `%x == 0`, `%lhs == 0x7fffffff`,
`fshl(0x7fffffff, 0, 30) == 0xc0000000`, and adding `%x` should leave the
result unchanged.

## Root Cause Notes

The ROCm 7.2.3 `-O2` output combines the product and the later `fshl`/add
chain into byte permutations plus a `v_dot4_u32_u8` with an accumulator of
`-1`:

```asm
v_lshl_or_b32 v0, s2, 8, v0
v_perm_b32 v4, v0, v0, s1
v_perm_b32 v5, s0, s0, v5
v_dot4_u32_u8 v4, v5, v4, -1
global_store_dword v[0:1], v4, off
```

For the zero input this stores `0xffffffff`. The `-O0` lowering keeps the
rotate/shift structure and stores `0xc0000000`. This points at the AMDGPU
`-O2` combine that recognizes the workitem-product plus `fshl` expression and
rewrites it as a byte dot product.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0xc0000000`, `O2=0xffffffff`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Passes: `O0=0xc0000000`, `O2=0xc0000000`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Passes: `O0=0xc0000000`, `O2=0xc0000000`. |

Original fuzzer input SHA-1:

```text
8e974a00d6a31fbcc7a5258309b1f514f80170e9
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer used to suppress generated `add(fshl(Y, x, 30), x)`
shapes. That suppression was removed after llvm/llvm-project#198412 fixed this
case for the active LLVM HEAD and ROCm HEAD campaigns.
