# MachineCSE: ImplicitDefsToUpdate positional indexing assumes operand parity

File: `llvm/lib/CodeGen/MachineCSE.cpp:621-689`

## Reasoning

`ProcessBlockCSE` walks MI's operands and, for each implicit *def* that's
not dead at MI, marks `CSMI->getOperand(i)` as not-dead via index `i`:

```cpp
for (unsigned i = 0, e = MI.getNumOperands(); NumDefs && i != e; ++i) {
  MachineOperand &MO = MI.getOperand(i);
  ...
  if (MO.isImplicit() && !MO.isDead() && CSMI->getOperand(i).isDead())
    ImplicitDefsToUpdate.push_back(i);
  ...
}
...
for (unsigned ImplicitDefToUpdate : ImplicitDefsToUpdate)
  CSMI->getOperand(ImplicitDefToUpdate).setIsDead(false);
for (const auto &PhysDef : PhysDefs)
  if (!MI.getOperand(PhysDef.first).isDead())
    CSMI->getOperand(PhysDef.first).setIsDead(false);
```

The pass assumes `MI` and `CSMI` have **identical operand layouts** because
they were value-numbered as equivalent. That's almost always true, but:

1. The value-numbering hash (`MachineInstrExpressionTrait`) considers operands but
   not implicit-operand *order*. Two semantically-equivalent variants that have
   different optional implicit-def lists (e.g., one with a `dead` flag stripped
   by a previous pass, or different number of regmask operands), will hash equal
   only if their MO count matches — but a subtle invariant.

2. More concretely: on x86, certain instructions (NF-ADD vs ADD, ZU-SETCC vs
   SETCC, COPY with reg-class changes) can produce operand lists where the
   implicit `$eflags`-def appears at different indices. When CSE folds the
   later occurrence into the earlier one, propagating "not dead" to the
   wrong-indexed CSMI operand silently corrupts liveness for an unrelated
   physreg implicit-def (e.g. clearing dead on `$rflags` would be fine but if
   index `i` lands on a register operand it would not.)

3. The same positional assumption applies to the PhysDefs loop:
   `CSMI->getOperand(PhysDef.first).setIsDead(false)` uses MI's operand index
   to update CSMI.

## Repro sketch

A MIR test mixing NF and non-NF arithmetic variants of the same op, where one
form emits `implicit-def $eflags` and the other doesn't, can be tricky to
construct since `isCSECandidate` requires identical opcode. The vulnerability
is real when two MIs of identical opcode were created at different times and
have differing operand-list layouts (e.g., one has had an implicit-def removed
by a peephole, the other still carries it).

Expected wrong outcome: an implicit physical-register def's dead flag flips
incorrectly, after which downstream liveness-using passes (LiveIntervals,
RA, post-RA scheduling) make incorrect decisions and a later reader of that
physreg gets stale data.

## Severity

Mostly a robustness/latent concern — the positional-equivalence invariant
holds for the current set of instructions because `MachineInstrExpressionTrait::isEqual`
compares all operands. But the explicit "go through implicit defs of CSMI and
MI" comment at line 631 misleads — the code does **not** go through implicit
defs of CSMI separately; it only re-indexes MI's. If MI has fewer operands
than CSMI, the loop simply terminates early, leaving CSMI's later
implicit-defs untouched.
