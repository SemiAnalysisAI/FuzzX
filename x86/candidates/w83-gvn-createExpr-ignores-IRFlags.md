# w83 GVN: `ValueTable::createExpr` ignores `IRFlags` (`nsw`/`nuw`/`disjoint`/`exact`/`inbounds`/FMF)

## Location
- `llvm/lib/Transforms/Scalar/GVN.cpp:329-374` (`createExpr`)

`createExpr` numbers an instruction by `(Ty, Opcode, VarArgs)` plus
attributes for calls. Notably absent: `IRFlags` (`nuw`/`nsw`), `disjoint`
on `or`, `exact` on `lshr`/`udiv`, `inbounds` on GEPs, and `FastMathFlags`
on FP ops. That means two BinaryOps with mismatched poison-generating
flags get the same value number, and the redundant one is replaced.
`patchAndReplaceAllUsesWith` strips the flags on the kept instruction so
the resulting IR is sound, but the *more refined* fact is lost.

## Reproducer

```ll
define i32 @gvn_or_disjoint(i32 %a, i32 %b) {
  %x = or disjoint i32 %a, %b
  %y = or i32 %a, %b
  %add = add nuw i32 %x, 0
  %z = xor i32 %y, %x
  ret i32 %z
}
```

`opt -passes=gvn -S` yields:

```ll
define i32 @gvn_or_disjoint(i32 %a, i32 %b) {
  %x = or i32 %a, %b      ; <-- 'disjoint' silently dropped
  ret i32 0
}
```

Same effect for `add nsw`/`add` pairing, etc.

## Severity caveat

This is strictly *flag-stripping*, which is safe in isolation. But there
are paths where the order of seen instructions decides which flag set
"wins" - meaning later passes may or may not see the more-refined fact
depending on how blocks are ordered, defeating optimizations that the
programmer/frontend tried to preserve. It is also a hazard for any
future pass that tries to preserve flags during CSE - the API gives no
indication that flags were merged.

## Suggested fix

Mirror `EarlyCSE`/`NewGVN` and include the IRFlags-relevant bits in the
expression's hash key when the opcode supports poison-generating flags.
At minimum, when the two instructions are merged, take the intersection
of flags rather than dropping unconditionally - `combineFlagsForReplace`
or equivalent.

## opt diff summary

Hand-verified via:
- `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -passes=gvn -S /tmp/w83_test_flag_disjoint.ll`
- `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -passes=gvn -S /tmp/w83_test_nuw.ll`

## x86 backend visibility

llc downstream sees the stripped flags, so any scheduling/lowering
heuristic gated on `nsw` / `disjoint` (e.g. `LEA` formation choices for
`add nsw`) silently degrades.
