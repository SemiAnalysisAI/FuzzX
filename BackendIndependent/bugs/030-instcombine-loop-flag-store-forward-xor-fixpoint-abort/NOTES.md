# 030 - alloca store forwarding exposes `X | ~X`

Component: `llvm/test/Transforms/InstCombine/pr142518.ll`

InstCombine forwards the alloca-backed `%flag` load, making the loop exit
constant. That exposes the boolean identity `or %cmp, (xor %cmp, true)`, which
is only folded on a second InstCombine iteration. With fixpoint verification
enabled, the first pass aborts.

Verifier: Boyle (019e994a-25b3-71f1-8911-20463123580c) returned YES.
