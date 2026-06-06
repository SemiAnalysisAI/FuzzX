# 029 - sunk instructions are revisited before DomConditionCache is populated

Component: `llvm/test/Transforms/InstCombine/sink_instruction.ll`

The upstream test explains the root cause: after InstCombine sinks the add and
divide into the guarded block, the sunk instructions are revisited before the
dominating condition is visited and entered in `DomConditionCache`. A later
iteration has stronger facts and changes the IR again, tripping the
assertion-enabled fixpoint verifier.

Verifier: Hilbert (019e994a-26a5-7b10-a8ec-3a77b4b7c89f) returned YES.
