# w51: Utils.cpp canCreateUndefOrPoison treats G_SDIV/G_UDIV/G_SREM/G_UREM as poison-safe

File: `llvm/lib/CodeGen/GlobalISel/Utils.cpp`
Function: `canCreateUndefOrPoison` (lines 1795-1900)

## Bug

The opcode switch in `canCreateUndefOrPoison` does not list `G_SDIV`,
`G_UDIV`, `G_SREM`, `G_UREM`. The `default:` arm at line 1897-1898 is:

```cpp
default:
  return !isa<GCastOp>(RegDef) && !isa<GBinOp>(RegDef);
```

`GBinOp::classof` (GenericMachineInstrs.h:707-746) explicitly includes
`G_SDIV`, `G_UDIV`, `G_SREM`, `G_UREM`. So the function returns **false**
for every division/remainder — i.e. "this can NOT create poison/undef."

IR (and GMIR by extension) semantics require:
- `udiv x, 0` -> poison
- `sdiv x, 0` -> poison
- `urem x, 0` -> poison
- `srem x, 0` -> poison
- `sdiv INT_MIN, -1` -> poison
- `srem INT_MIN, -1` -> poison

## Impact

`isGuaranteedNotToBeUndefOrPoison` at line 1944 calls
`!::canCreateUndefOrPoison(...)`. If a G_UDIV result is then fed into
a `freeze` removal, a `select`-of-poison fold, a branch-condition fold,
or any combiner that needs "guaranteed not poison," that combiner will
treat the divide result as guaranteed-defined and may speculatively
hoist, branch on, or duplicate it.

Concretely: combiners that ask "is X guaranteed not poison?" before
substituting it across control-flow boundaries will say YES for
`%q = G_UDIV %x, %y` even when `%y` may be zero, then freely speculate
or freeze-remove the divide. The resulting machine code may execute a
divide on a path where the source IR never would (i.e. introduce a
UB-trap that didn't exist in the original program).

## Comparison to LLVM IR helper

`llvm::canCreateUndefOrPoison` in `llvm/lib/Analysis/ValueTracking.cpp`
correctly handles UDiv/SDiv/URem/SRem with explicit divisor-known-nonzero
checks (and INT_MIN/-1 check for SDiv/SRem). The GMIR helper is missing
both checks.

## Fix sketch

Add explicit cases in the switch, e.g.:

```cpp
case TargetOpcode::G_UDIV:
case TargetOpcode::G_UREM:
  return !isKnownNeverZero(RegDef->getOperand(2).getReg(), MRI);
case TargetOpcode::G_SDIV:
case TargetOpcode::G_SREM:
  // Poison if divisor is 0, or if dividend==INT_MIN and divisor==-1.
  return /* conservative */ true;
```

or at minimum drop the `!isa<GBinOp>(RegDef)` clause from the default arm.

## Status

Source-confirmed structural bug; no x86 runtime repro attempted (would
need a target-specific combiner that consumes `isGuaranteedNotToBeUndefOrPoison`
on a divide result — AMDGPU and AArch64 GISel post-legalizer combiners
are the most likely first observers, but x86 GISel may also trip it via
shared CombinerHelper paths). Strong confidence in source analysis.
