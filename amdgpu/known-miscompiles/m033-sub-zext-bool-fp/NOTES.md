# m033: scalar `sub` of zexted boolean miscompiled through `s_subb_u32`

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after adding half-precision generation. The reduced testcase no longer
needs half operations; the live trigger is a scalarized boolean subtract feeding
a masked FP accumulation tail.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m033-sub-zext-bool-fp/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0x00000000
O2=0x000003ff
mismatch=true
```

## Reduction

For the reproducer input, `%n == 1` and `%v == 0`. The select computes zero,
so `%a == 0`, `%product.i == 0`, `%same == false`, `%same.i == 0`, and
`%sub == 0`. The final masked FP sum is therefore also zero:

```llvm
%x = select i1 (icmp slt i32 %n, 0), i32 %n, i32 0
%a = and i32 %x, 1023
%product.i = fptoui float (fmul (uitofp %a), (uitofp (%v & 1023))) to i32
%same = icmp ne i32 %a, %v
%same.i = zext i1 %same to i32
%sub = sub i32 %v, %same.i
%sub.masked = and i32 %sub, 1023
```

## Root Cause Notes

The ROCm 7.2.3 `-O2` output scalarizes the path and lowers the boolean
subtract through a compare plus `s_subb_u32`:

```asm
s_cmp_lg_u32 s1, s4
s_cselect_b64 s[0:1], -1, 0
s_cmp_lg_u64 s[0:1], 0
s_subb_u32 s0, s4, 0
s_and_b32 s0, s0, 0x3ff
```

For `%a == %v == 0`, `%same.i` should be zero and `%v - %same.i` should stay
zero. The generated `s_subb_u32` sequence instead subtracts a borrow in the
false case, producing `0xffffffff`; the following mask turns that into
`0x000003ff`. The `-O0` code materializes the boolean as `0` or `1` with
`v_cndmask_b32` and then uses a normal `v_sub_u32`, so it stores zero.

This points at the scalar boolean-to-borrow lowering for
`sub i32 X, zext(i1 Cond)` when the result remains in a value-producing FP
accumulation chain.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000000`, `O2=0x000003ff`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

Original fuzzer input SHA-1:

```text
a59fa8dbcd842a07230ba2100053f1b247c0be83
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer used to suppress generated `sub i32 X, zext(i1 Cond)`
shapes. That suppression was removed after llvm/llvm-project#198412 fixed this
case for the active LLVM HEAD and ROCm HEAD campaigns.
