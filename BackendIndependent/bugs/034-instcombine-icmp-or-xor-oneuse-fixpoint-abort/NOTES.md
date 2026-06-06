# 034 - one-use precondition changes after a sibling compare folds

Component: `llvm/test/Transforms/InstCombine/icmp-or.ll`

`foldICmpOrXorSubChain` only takes the complete simplification when the xor has
one use. In this reproducer the first compare sees the xor with two uses. After
the sibling compare folds, the xor becomes one-use, but the earlier consumer is
not revisited until a second InstCombine iteration.

Verifier: Sagan (019e9951-0b49-7c90-94c7-6c4e604f57d9) returned YES.
