# 032 - delayed logical-select to bitwise-and relaxation

Component: `llvm/test/Transforms/InstCombine/shift.ll`

This is the upstream `ashr_out_of_range` fixpoint-suppressed test with the
suppression removed. InstCombine first canonicalizes a logical form involving a
select, but only a later iteration relaxes it to a bitwise `and`, so the first
run does not reach a fixpoint.

Verifier: Hypatia (019e994b-a21e-7cb1-90f1-7728a1336b93) returned YES.
