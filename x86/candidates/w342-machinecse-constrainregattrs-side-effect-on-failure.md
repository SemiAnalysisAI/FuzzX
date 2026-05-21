# w342: MachineCSE leaves NewReg with narrowed reg class after a failed CSE attempt

## Summary
When `ProcessBlockCSE` walks the def operands of an MI to set up CSE pairs, it
calls `MRI->constrainRegAttrs(NewReg, OldReg)` for each def. `constrainRegAttrs`
**mutates** `NewReg` -- it can narrow `NewReg`'s reg class and/or set
`NewReg`'s LLT -- and returns false if some operand can't be unified. If the
first def succeeds (so `NewReg`'s class/type are modified) but a *later* def
operand causes the call to return false, MachineCSE sets `DoCSE = false`,
`break`s out, and walks on -- but the side-effecting class/type changes on the
earlier `NewReg`(s) are *not* reverted. `NewReg` is now defined by `CSMI` with
a narrower class than before, even though no CSE happened.

## Root cause (source citations)

`llvm/lib/CodeGen/MachineCSE.cpp:649-666`:
```cpp
if (!isProfitableToCSE(NewReg, OldReg, CSMI->getParent(), &MI)) {
  DoCSE = false; break;
}

// Don't perform CSE if the result of the new instruction cannot exist
// within the constraints (register class, bank, or low-level type) of
// the old instruction.
if (!MRI->constrainRegAttrs(NewReg, OldReg)) {
  DoCSE = false; break;
}

CSEPairs.emplace_back(OldReg, NewReg);
--NumDefs;
```

`llvm/lib/CodeGen/MachineRegisterInfo.cpp:90-117` `constrainRegAttrs`:
```cpp
const auto &RegCB = getRegClassOrRegBank(Reg);
if (RegCB.isNull())
  setRegClassOrRegBank(Reg, ConstrainingRegCB);      // <-- side effect
...
::constrainRegClass(*this, Reg, ..., ConstrainingRegCB, MinNumRegs)
// `constrainRegClass` mutates Reg's class as it walks intersections
...
if (ConstrainingRegTy.isValid())
  setType(Reg, ConstrainingRegTy);                    // <-- side effect
```

So when MI has N defs (N > 1) and the constrain succeeds for the first
def-pair `(OldReg_0, NewReg_0)` but fails for `(OldReg_1, NewReg_1)`,
`NewReg_0`'s register class and type have already been narrowed by the
intersection with `OldReg_0`'s constraints, even though the surviving CSMI is
left alone (no CSE happened, CSEPairs were not applied). Any later use of
`NewReg_0` in the function is now in a more constrained class than it
originally needed, which can:
- prevent later passes from re-using `NewReg_0` in a wider context, or
- cause regalloc to spill more aggressively (NewReg_0 in a narrower class has
  fewer available physregs).

## How often this can fire
GISel multi-def instructions (`G_UNMERGE_VALUES`, `G_UADDO`, ...), x86 multi-def
post-RA expansions (e.g. `IDIV` which defs both quotient and remainder pre-RA
through `GR32`/`GR64` reg pairs), and any AArch64/AMDGPU instruction with two
or more vreg defs will hit this code path if MachineCSE finds a match for the
first def but the second def's reg class is incompatible with CSMI's.

## Reproducer setup
Construct a `G_UNMERGE_VALUES`-like MIR where CSMI defines `%csmi_lo:gr64,
%csmi_hi:gr32` and MI defines `%mi_lo:gr64, %mi_hi:gr8`. `isIdenticalTo`
matches on the operands (modulo vreg-def-ignore). First operand constrain
succeeds. Second operand `constrainRegAttrs(NewReg_hi=gr32, OldReg_hi=gr8)`
fails because the bank/class is incompatible. We bail out with DoCSE=false,
but `%csmi_lo` was already narrowed to gr64's intersection with `%mi_lo`'s
constraints (could be a sub-class like gr64_norex2 if MI's user constrained
the operand). This narrowed class then propagates to all uses of CSMI's def.

Note: x86 -O2 default has few multi-def x86 instructions that survive to
machine-cse, but the same pattern exists in GISel paths used by other targets
included in the build.

## Fix sketch
- Either save and restore the (class, bank, LLT) of every NewReg touched
  before each `constrainRegAttrs` call, and restore on failure; or
- pre-check compatibility for ALL def pairs first (using a side-effect-free
  predicate that mirrors the constrainRegAttrs intersection), and only call
  the mutating version once we know every pair will succeed.

## Cite
- `llvm/lib/CodeGen/MachineCSE.cpp:649-666` (loop that calls
  `constrainRegAttrs` and breaks on failure without rollback)
- `llvm/lib/CodeGen/MachineRegisterInfo.cpp:90-117` (constrainRegAttrs side
  effects on Reg)
- `llvm/lib/CodeGen/MachineRegisterInfo.cpp:69-87` (constrainRegClass mutates
  RegClass via setRegClass on intersection)
