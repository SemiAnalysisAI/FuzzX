# 001 - InstCombine aborts after poison-store deletion exposes a second fold

Component: `llvm/lib/Transforms/InstCombine`

The IR verifies, but assertion-enabled `opt -passes=instcombine` aborts with:

```text
LLVM ERROR: Instruction Combining on autogen_SD232 did not reach a fixpoint after 1 iterations.
```

The first InstCombine iteration removes `store i8 poison` as a no-op. That
deletion exposes a later load/store fold on the fixpoint-check iteration: the
load can be sunk into the successor and the `load; store same value` pair can
be removed. `instcombine<max-iterations=2>` reaches the final form cleanly.

This is a pass assertion failure rather than a value miscompile. It reproduces
with ordinary integral pointers and does not depend on non-integral pointer
representations.

Verifier: Harvey (019e98e1-d4db-7ee1-b623-267782673b4f) returned YES.
