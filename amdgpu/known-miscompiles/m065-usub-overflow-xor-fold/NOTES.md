# m065: `(lane ^ fold) & 1` after `usub.with.overflow` returns the wrong bit at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#196418,
llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508,
and llvm/llvm-project#198556 applied.  The original oracle finding was:

```text
kind=oracle
index=12
input=0x3E9D7382
o0=0xB933EE00
o2=0xB933EE01
expected=0xB933EE01
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m065-usub-overflow-xor-fold/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches and on ROCm HEAD:

```text
[0] input=0x00000000 O0=0x00000000 O2=0x00000001 mismatch=true
[1] input=0x00000000 O0=0x00000000 O2=0x00000001 mismatch=true
[2] input=0x00000000 O0=0x00000000 O2=0x00000001 mismatch=true
any_mismatch=true
```

ROCm 7.2.3 passes (`O0=O2=1` for the same kernel), so this is a HEAD
regression introduced after the 7.2.3 release.

## Reduction

The reproducer was distilled by hand from a 470-line `ovbytegather.idiom`
finding (`fuzzx-amdgpu-diff-1779213770-998025`).  `llvm-reduce` was unable to
shrink past ~147 lines because its operand-skip pass kept changing the kernel
signature instead of cutting the byte-gather chain.  The hand-reduced kernel
keeps only the operations needed to reproduce the bit-0 mismatch:

```llvm
%ov.call  = call { i32, i1 } @llvm.usub.with.overflow.i32(i32 %x.lo,
                                                          i32 %y.nz)
%ov.value = extractvalue { i32, i1 } %ov.call, 0
%ov.bit   = extractvalue { i32, i1 } %ov.call, 1
%ov.i32   = zext i1 %ov.bit to i32
%lane.xor = xor i32 %ov.value, %ov.i32
%fold.add = add i32 0, %lane.xor
%fold     = xor i32 %fold.add, %ov.i32
%byte.xor = xor i32 %lane.xor, %fold
%byte     = and i32 %byte.xor, 1
store i32 %byte, ptr addrspace(1) %out.ptr, align 4
```

For every workitem `wi`, `%y.nz = (wi & 0xff) | 1 >= 1`, so the unsigned
subtraction `%x.lo - %y.nz` underflows whenever `%x.lo == 0` (which is what
the test inputs guarantee).  Substituting `ov.bit = 1` and reducing the
algebra:

* `lane.xor = ov.value ^ 1`
* `fold     = (lane.xor) ^ 1 = ov.value`
* `byte.xor = lane.xor ^ fold = 1`
* `byte     = 1 & 1 = 1`

So the store must be `1`.  ROCm 7.2.3 and `-O2` agree; LLVM HEAD / ROCm HEAD
`-O0` store `0` instead.

## Root Cause Notes

The `-O0` lowering folds the final `(lane.xor ^ fold) & 1` into a single
`v_bitop3_b32` instruction whose operands and truth table are inconsistent
with the IR:

```asm
v_sub_co_u32_e64 v2, s[2:3], v2, v3        ; v2 = ov.value, s[2:3] = ov.bit
v_cndmask_b32_e64 v3, 0, 1, s[2:3]         ; v3 = ov.i32
v_xor_b32_e64    v2, v2, v3                ; v2 = lane.xor
v_xor_b32_e64    v3, v2, v3                ; v3 = fold = lane.xor ^ ov.i32
v_bitop3_b32     v2, v2, s0=1, v3          ; v2 = bitop3(lane.xor, 1, fold)
global_store_dword v[0:1], v2, off
```

`v3` is overwritten in place by the second `v_xor_b32`, so the inputs to
`v_bitop3_b32` are `(lane.xor, 1, fold)`.  The expected truth table for
`(operand0 ^ operand2) & operand1` should give bit 0 = 1, but the value
produced by the bitop3 instruction is 0 — pointing at a wrong truth-table
constant emitted by the `-O0` selection path for this exact `xor`-of-`xor`
pattern after the in-place clobber.  This appears related to but not
suppressed by llvm/llvm-project#198556.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=O2=0x1`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#196418, llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508, llvm/llvm-project#198556 applied locally | Reproduces: `O0=0x0`, `O2=0x1`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with the same five PR patches applied locally | Reproduces: `O0=0x0`, `O2=0x1`. |

Original fuzzer input SHA-1:

```text
e009af75455dcdbfafddb276110bc88e8c8e25ee
```

Reduced reproducer SHA-1:

```text
842f905d5c58a15782e0f0a364f8e430f9161e49
```

## Fuzzer Follow-Up

The fuzzer now rejects final stores whose lowest byte depends on
`(extractvalue overflow-call, 0) ^ (extractvalue overflow-call, 1) ^
(self ^ extracted-overflow)` cascades by default.  Set
`FUZZX_ALLOW_M065_USUB_OVERFLOW_XOR_FOLD=1` to re-enable this bug class.
