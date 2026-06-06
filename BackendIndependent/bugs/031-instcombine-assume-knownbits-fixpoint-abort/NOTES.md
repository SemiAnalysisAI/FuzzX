# 031 - same-block assume changes known bits after an earlier consumer ran

Component: `llvm/test/Transforms/InstCombine/zext-or-icmp.ll`

InstCombine creates an `llvm.assume` involving the negated compare. On the next
iteration, that same-block assume feeds known-bits reasoning for an earlier
value, allowing additional simplification that was missed in the first
iteration. Assertion-enabled InstCombine therefore aborts as not reaching a
fixpoint.

Verifier: Socrates (019e994b-a14c-7891-8644-d6ae148f785f) returned YES.
