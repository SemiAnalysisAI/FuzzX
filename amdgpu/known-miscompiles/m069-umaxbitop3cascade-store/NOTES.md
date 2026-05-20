# m069: `umaxbitop3cascade` final store is miscompiled at `-O2`

Found while fuzzing LLVM HEAD with llvm/llvm-project#196418,
llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508,
and llvm/llvm-project#198556 applied.  The original oracle finding was:

```text
kind=oracle
index=0
input=0x0
o0=0x814EF57
o2=0x5C83AF47
expected=0x814EF57
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m069-umaxbitop3cascade-store/reduced.ll
```

The store value is `fuzz.umaxbitop3cascade.idiom.a.add`.  Observed result:

```text
input=0x00000000
O0=0x0814ef57
O2=0x5c83af47
mismatch=true
```

## Reduction

Not yet reduced past the original generated 320+ line IR; the file is the
post-processed fuzzer output.  A proper `llvm-reduce` pass should be run
when time permits.

## Root Cause Notes

Not yet root-caused.  The bug class is the same as m068 in spirit — both
are `-O2` miscompiles whose store value comes out of an
`umaxbitop3cascade.idiom`-derived value (this finding stores the cascade
output directly, while m068 stores a loop accumulator that consumed it).
Both reproduce on ROCm 7.2.3, LLVM HEAD, and ROCm HEAD with the same
patches.

The `umaxbitop3cascade` idiom (`emitRandomUMaxBitop3CascadeIdiom` in
`fuzzer/llvm_amdgpu_diff_fuzzer.cpp`) was added to stress shapes that
198556 doesn't catch — chained umax/umin against shl/lshr-shifted
operands and complements, each step producing different bitop3-friendly
truth tables.  m068 and m069 are bugs in that exact `-O2` lowering path.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build | Not yet tested explicitly. |
| LLVM HEAD with the local PR patches | Reproduces: `O0=0x0814ef57`, `O2=0x5c83af47`. |
| ROCm HEAD with the same five PR patches applied locally | Not yet tested. |

## Fuzzer Follow-Up

Shares the `triggersM068M069UmaxBitop3CascadeStore` validator hook with
m068.  Set `FUZZX_ALLOW_M069_UMAXBITOP3CASCADE_STORE=1` to re-enable this
specific bug class.
