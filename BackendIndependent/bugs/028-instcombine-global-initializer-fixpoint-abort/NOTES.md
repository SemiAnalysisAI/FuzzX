# 028 - global initializer folding exposes a second InstCombine iteration

Component: `llvm/test/Transforms/InstCombine/pr55228.ll`

This reproducer removes the upstream test's
`instcombine-no-verify-fixpoint` suppression. InstCombine forwards through a
zero-length memcpy from a constant global initializer, but the initializer is
not in the canonical folded form that the later compare fold expects. A second
iteration changes the function again, so an assertion-enabled `opt` aborts with
the InstCombine fixpoint verifier.

This is an ordinary integer-pointer assertion failure, not a non-integral
pointer representation issue.

Verifier: Ohm (019e994a-254b-7750-bb92-50cf9a0175f2) returned YES.
