# 025 - Wide load/compare fold exposes duplicate-store DSE after the fixpoint check

Component: `llvm/lib/Transforms/InstCombine`

The IR verifies, but assertion-enabled `opt -passes=instcombine` aborts with:

```text
LLVM ERROR: Instruction Combining on wide_load_compare_fixpoint did not reach a fixpoint after 1 iterations.
```

The first InstCombine iteration folds the wide load/compare result to a
constant splat. That exposes two identical stores to the same pointer, so a
second InstCombine iteration removes the first store. The default
assertion-enabled fixpoint verifier detects that the first iteration was not a
fixpoint and aborts.

This is a pass assertion failure rather than a value miscompile. It is
ordinary-pointer IR and does not use poison stores, so it is distinct from
`001`.

Verifier: Confucius (019e992e-168c-7393-845b-2b9427e8342b) returned YES.
