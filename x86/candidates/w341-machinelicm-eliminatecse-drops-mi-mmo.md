# w341: MachineLICM::EliminateCSE drops eliminated MI's MMO

## Summary
The CSE that MachineLICM runs after hoisting (`EliminateCSE`) is structurally
the same bug as w340 but in a different pass. When a hoisted `MI` matches a
`Dup` already in the preheader via `LookForDuplicate`, only `MI`'s vreg defs
are replaced by `Dup`'s and then `MI->eraseFromParent()` -- `Dup`'s MMO is kept
as-is and `MI`'s MMO is discarded. Because `LookForDuplicate` calls
`TII->produceSameValue`, which defaults to
`MI0.isIdenticalTo(MI1, IgnoreVRegDefs)` and **does not look at MMOs**, two
loads that share opcode/operands but differ in `!range`, `!invariant.load`,
`!nontemporal`, or `AAInfo` (alias.scope / noalias / TBAA) are treated as
identical, and the stricter metadata can survive on `Dup` only by accident.

## Root cause (source citations)

- `llvm/lib/CodeGen/MachineLICM.cpp:1505-1564` `EliminateCSE`:
  - line 1515: rejects only non-invariant loads (correct);
  - line 1518: matches via `LookForDuplicate`;
  - line 1551-1559: replaces vreg defs of `MI` with those of `Dup`;
  - line 1561: `MI->eraseFromParent()` -- no merging of MMOs into `Dup`.
- `llvm/lib/CodeGen/MachineLICM.cpp:1491-1499` `LookForDuplicate` ->
  `TII->produceSameValue`.
- `llvm/lib/CodeGen/TargetInstrInfo.cpp:469-473` default
  `produceSameValue` just calls `isIdenticalTo(IgnoreVRegDefs)`.
- `llvm/lib/CodeGen/MachineInstr.cpp:673-740` `isIdenticalTo` does not check
  `memoperands()`.

## What can be lost
On the LICM path the MMOs that can differ between two otherwise-identical loads
that LICM hoists from different positions of the same loop body are:
- `MOInvariant` (`!invariant.load`)
- `MODereferenceable`
- `MONonTemporal` (`!nontemporal`)
- `AAInfo` (`!alias.scope`, `!noalias`, `!tbaa`, `!tbaa.struct`)
- `!range`
- alignment (kept-MMO's `Align` may be coarser than the dropped one's)

Worst case the AAInfo we lose is the only thing telling a later pass that the
load does not alias a store reachable from the preheader, so subsequent
`MachineSink::hasStoreBetween` or `mayAlias` queries (line 1675/1735 of
`MachineSink.cpp`) become more conservative and a profitable move is blocked.

The cleanest correctness concern is `!range`: a later consumer that calls
`MachineMemOperand::getRanges()` (e.g. `X86::isKnownNeverZero`-style hooks,
or a target's `tryFoldRangedLoadConstant`) sees only the survivor's range and
may fold based on a value that the eliminated MI's range said was different.

## Fix sketch
At line 1559 (right before the `MI->eraseFromParent()` at line 1561) call
`Dup->cloneMergedMemRefs(*MF, {Dup, MI})` so that the surviving instruction
carries the intersection-of-flags / union-of-AAInfo / union-of-ranges of both
sources.

## Cite
- `llvm/lib/CodeGen/MachineLICM.cpp:1505-1565` (`EliminateCSE`)
- `llvm/lib/CodeGen/MachineLICM.cpp:1491-1499` (`LookForDuplicate`)
- `llvm/lib/CodeGen/TargetInstrInfo.cpp:469-473` (`produceSameValue` default)
- `llvm/lib/CodeGen/MachineInstr.cpp:673-740` (`isIdenticalTo`)
- `llvm/lib/CodeGen/MachineInstr.cpp:2332-2345` (`getHashValue`)

## Notes
Same fix should be applied at every MI-replacement site that uses
`isIdenticalTo`/`produceSameValue` without merging MMOs. Beyond w340/w341 the
other call sites are `MachineSink::aggressivelySinkIntoCycle`
(`CloneMachineInstr` already copies MMOs, so that one is OK) and
`MIRCanonicalizer`'s rename pass (no MMOs touched).
