# m062: byte histogram low byte is miscompiled after a bitmux chain at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0xB81C0001
o2=0xB81C0002
expected=0xB81C0002
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m062-bytehist-bitmux-lowbyte/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x00000000 O2=0x00000000 mismatch=false
[1] input=0x00000001 O0=0xb81c0001 O2=0xb81c0002 mismatch=true
any_mismatch=true
```

The reproducer has `RUN-INPUTS: 0x0 0x1` because the reduced kernel's third
argument is still live in the computation. Passing only `0x1` changes that
argument and does not reproduce this exact lowered shape.

## Reduction

`llvm-reduce` reduced the original 348-line textual IR to a 97-line kernel. The
checked-in file has two extra `RUN-*` comments and removes the newer
`nocreateundeforpoison` attribute spelling so the same file can also be parsed
by the ROCm 7.2.3 source build.

The reduced kernel feeds a generated bitmux chain into a byte-histogram tail:

```llvm
%fuzz.bitmux.idiom.x.xor =
  xor i32 %fuzz.bitmux.idiom.fold.next56, %fuzz.bitmux.idiom.x.next57
%fuzz.bytehist.idiom.absdiff.absdiff =
  sub i32 %fuzz.bytehist.idiom.absdiff.hi.umax,
          %fuzz.bytehist.idiom.absdiff.lo.umin
store i32 %fuzz.bytehist.idiom.pack.add101, ptr addrspace(1) %out.ptr, align 4
```

## Root Cause Notes

For input lane 1, the LLVM interpreter and `-O2` GPU compile agree on
`0xb81c0002`. LLVM HEAD and ROCm HEAD `-O0` store `0xb81c0001`.

The reduced `-O0` assembly lowers the byte-histogram low-byte computation
through a `v_bitop3_b32` sequence before the compare/select/subtract tail:

```asm
v_bitop3_b32 v2, s3, v1, v2
v_cmp_gt_u32_e64 s[6:7], v2, s8
v_cndmask_b32_e64 v1, v1, v2, s[6:7]
v_cndmask_b32_e64 v2, v2, v3, s[6:7]
v_sub_u32_e64 v1, v1, v2
```

The optimized path computes the corresponding low byte as `2`, using scalar
`and` / `max` / subtract-with-borrow style code. This points to an `-O0`
lowering issue in the unoptimized bitmux-to-bytehist shape, not an IR
semantics issue.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: lane 1 `O0=0xb81c0002`, `O2=0xb81c0002`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0xb81c0001`, `O2=0xb81c0002`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0xb81c0001`, `O2=0xb81c0002`. |

Original fuzzer input SHA-1:

```text
4a1757cf6e5e541e4d2b1fe89e04dcebf2b3a87f
```

Reduced reproducer SHA-1:

```text
0ba84bb651b245a3f936d568cdec03f7ca851533
```

## Fuzzer Follow-Up

The fuzzer now rejects final stores depending on both generated `bytehist` and
`bitmux` values by default. Set `FUZZX_ALLOW_M062_BYTEHIST_BITMUX=1` to
re-enable this bug class.
