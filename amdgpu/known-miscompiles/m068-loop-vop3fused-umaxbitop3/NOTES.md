# m068: nested loop with `vop3fused` + `umaxbitop3cascade` is miscompiled at `-O2`

Found while fuzzing LLVM HEAD with llvm/llvm-project#196418,
llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508,
and llvm/llvm-project#198556 applied.  The original oracle finding was:

```text
kind=oracle
index=0
input=0x0
o0=0x8210A05D
o2=0x937E
expected=0x8210A05D
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m068-loop-vop3fused-umaxbitop3/reduced.ll
```

Observed result on all three of our toolchains (this is *not* a HEAD
regression — ROCm 7.2.3 reproduces too):

```text
input=0x00000000
O0=0x8210a05d
O2=0x0000937e
mismatch=true
```

## Reduction

This file is the original 382-line fuzzer output, lightly post-processed (added
`; RUN-LLVM-BUILD` + `; RUN-INPUTS` headers, renamed `fuzz_kernel_o0` ->
`fuzz_kernel`).  A proper `llvm-reduce` pass should be run when there is time;
the kernel is dominated by a nested loop whose body chains
`bitrunmask` / `cfg.clamppack` / `cfg.bool` / `cfg.avgdiff` / `bitcount` /
`cfg.bitdeposit` operations.  The prologue computes an initial value from
the existing `vop3fused.idiom` and `umaxbitop3cascade.idiom` generators.

## Root Cause Notes

Not yet root-caused.  The bug is in `-O2`, so loop unrolling + SROA + the
vectorization pipeline are likely involved.  Both `vop3fused` (which produces
explicit ADD3/LSHL_ADD/AND_OR/OR3/MED3/LSHL_OR shapes) and
`umaxbitop3cascade` (which produces nested umax/umin + xor+and+or shapes
that the bitop3 selector tries to fuse) are deliberate stress patterns
introduced by the generator batch covering AMDGPU's fused VOP3 selection.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Reproduces: `O0=0x8210a05d`, `O2=0x937e`. |
| LLVM HEAD with the local PR patches | Reproduces: same values. |
| ROCm HEAD with the same five PR patches applied locally | Reproduces: same values. |

## Fuzzer Follow-Up

Suppressed by the shared `triggersM068M069UmaxBitop3CascadeStore` validator
hook keyed on `fuzz.umaxbitop3cascade.idiom` SSA names appearing in the
store value's dependency graph.  Set
`FUZZX_ALLOW_M068_LOOP_VOP3FUSED_UMAXBITOP3=1` to re-enable this bug class.
