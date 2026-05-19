# m064: nibble-carry loop value is miscompiled at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0x1805D9
o2=0xC1B09
expected=0xC1B09
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m064-nibblecarry-loop-readfirstlane/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[0] input=0x00000000 O0=0x00120306 O2=0x00120306 mismatch=false
[1] input=0x00000001 O0=0x001805d9 O2=0x000c1b09 mismatch=true
any_mismatch=true
```

## Reduction

`llvm-reduce` reduced the original 718-line generated IR to 586 lines before a
15-minute bounded reducer run stopped. The checked-in file has two extra
`RUN-*` comments and removes the newer `nocreateundeforpoison` attribute
spelling so the ROCm 7.2.3 source build can parse it too.

The reduced kernel keeps an i64 prefix scan, a bit-matrix pack, a vector i8
shuffle/multiply pack, a nibble-carry chain, and a loop-carried final value:

```llvm
%fuzz.nibblecarry.idiom.fold.sub =
  sub i32 %fuzz.nibblecarry.idiom.fold.next106,
          %fuzz.nibblecarry.idiom.pack.add114
%fuzz.loop.nest.acc =
  phi i32 [ %fuzz.nibblecarry.idiom.fold.sub, %body ],
          [ %fuzz.loop.acc.inner, %fuzz.nested.loop.exit ]
store i32 %fuzz.loop.nest.acc, ptr addrspace(1) %out.ptr, align 4
```

## Root Cause Notes

For lane 1, the LLVM interpreter and the `-O2` GPU compile agree on
`0x000c1b09`. LLVM HEAD and ROCm HEAD `-O0` store `0x001805d9`.

The reduced `-O0` assembly scalarizes a divergent loop-carried value around the
nested loop setup:

```asm
v_sub_u32_e64 v0, v0, v1
...
v_readfirstlane_b32 s2, v0
...
s_cbranch_vccnz .LBB0_3
```

The final store then writes the loop accumulator value:

```asm
v_accvgpr_read_b32 v2, a10
global_store_dword v[0:1], v2, off
```

The optimized path keeps the lane-local arithmetic through the final sequence
and stores the oracle result. This points to an `-O0` lowering issue involving
a divergent nibble-carry-derived loop value, with `v_readfirstlane_b32` in the
bad path, rather than an IR semantics issue.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: lane 1 `O0=0x000c1b09`, `O2=0x000c1b09`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x001805d9`, `O2=0x000c1b09`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0x001805d9`, `O2=0x000c1b09`. |

Original fuzzer input SHA-1:

```text
88897612925eaed56bb99a5ae24840dd6fbad998
```

Reduced reproducer SHA-1:

```text
e237810a4dc658cc82f15537a91108e78068ed1c
```

## Fuzzer Follow-Up

The fuzzer now rejects loop-carried final stores depending on generated
`nibblecarry` values by default. Set `FUZZX_ALLOW_M064_NIBBLECARRY_LOOP=1` to
re-enable this bug class.
