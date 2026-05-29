# 116 — `SelectionDAG::simplifyFPBinop` identity folds bypass sNaN quieting (fadd X, -0.0 / fsub X, +0.0)

Companion of #115 for the additive arms of `simplifyFPBinop`. Both lower to
bare `retq` — no `addss`/`subss` against the constant zero is emitted, so the
sNaN quieting that the hardware performs is silently elided.

`./cmd.sh` shows both `fadd_neg0` and `fsub_pos0` reduce to a single `retq`.
