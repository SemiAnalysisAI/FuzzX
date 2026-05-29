# 261 — LICM hoistAdd/hoistSub keep `samesign` on a reassociated icmp (miscompile)

Component: `llvm/lib/Transforms/Scalar/LICM.cpp` `hoistAdd` / `hoistSub`.

These reassociate `LV + C1 <pred> C2` → `LV <pred> (C2 - C1)`, replacing the
icmp's LHS via `setPredicate`/`setOperand` — but never clear the `samesign`
flag. `samesign` asserts `sign(LHS) == sign(RHS)`; with a new LHS that no longer
holds, so a defined comparison becomes poison.

## Miscompile (verified at HEAD via x86 execution)
`icmp samesign slt i32 (add nsw %iv,5), 100` → `icmp samesign slt i32 %iv, 95`.
For `%iv = -3`: source `samesign slt(2, 100)` (both ≥0) is defined-true, but the
reassociated `samesign slt(-3, 95)` has opposite-sign operands → poison; a legal
consumer realizing `samesign slt` as `ult` yields `ult(0xFFFFFFFD,95)=false`.
Rosetta x86 run: source=1, transformed-realization=0 — wrong value introduced.

## Fix
PR [#200344](https://github.com/llvm/llvm-project/pull/200344): `ICmp.dropPoisonGeneratingFlags()`
after the operand rewrite in both `hoistAdd` and `hoistSub` (as
`hoistMulAddAssociation` already does).
