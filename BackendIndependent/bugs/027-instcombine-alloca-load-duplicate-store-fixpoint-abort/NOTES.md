# 027 - Alloca load forwarding exposes duplicate-store DSE after the fixpoint check

Component: `llvm/lib/Transforms/InstCombine`

The IR verifies, but assertion-enabled `opt -passes=instcombine` aborts with:

```text
LLVM ERROR: Instruction Combining on alloca_load_exposes_duplicate_store did not reach a fixpoint after 1 iterations.
```

The first iteration forwards the load from the alloca-backed constant store and
turns the return into `ret i16 0`. Once that fold happens, the two identical
stores to `%p` are exposed and a second InstCombine iteration removes the first
one. The default fixpoint verifier therefore aborts.

This is a pass assertion failure rather than a value miscompile. It is distinct
from `001` and `025`: no poison store is involved, and the exposing fold is
alloca load/store forwarding rather than a wide load/compare simplification.

Verifier: Peirce (019e9934-87cb-78d0-8cf5-3e60240e4c0a) returned YES.
