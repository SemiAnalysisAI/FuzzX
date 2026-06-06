# 033 - bounded available-load scan misses CSE until the block is shortened

Component: `llvm/test/Transforms/InstCombine/and-or-icmps.ll`

The first iteration canonicalizes and shortens the block. Only after that does
the bounded available-load scan see that an earlier load can be reused, which
unlocks another fold. With the upstream fixpoint suppression removed,
assertion-enabled InstCombine aborts.

Verifier: Helmholtz (019e994b-a0e1-7220-9968-fbb4e97e5daf) returned YES.
