# w344: MachineCSE::PerformTrivialCopyPropagation narrows SrcReg via constrainRegAttrs, then keeps DefMI alive

## Summary
`PerformTrivialCopyPropagation` walks the uses of MI looking for vreg uses
whose defining copy can be propagated through. For each such use, it calls
`MRI->constrainRegAttrs(SrcReg, Reg)` BEFORE rewriting the use. If
`constrainRegAttrs` returns true, the side-effecting register-class /
LLT narrowing on `SrcReg` is already done by then. The code then commits to
the rewrite (`MO.setReg(SrcReg)`, etc.). So far, so good.

The bug is in the surrounding loop: the function iterates `MI->all_uses()`
and may call `constrainRegAttrs` for several different SrcRegs. If a LATER
iteration's `constrainRegAttrs` fails (the failure path is `continue`, not
`break`/`return`), the EARLIER iteration's narrowing on its SrcReg is left
in place but the source COPY (`DefMI`) is also left alive (deletion only
happens when `OnlyOneUse` and we commit). Subsequent uses of that earlier
SrcReg elsewhere in the function are now in a narrower reg class than they
needed -- and the still-alive COPY may produce a value in the WIDER class,
mismatching what the regalloc sees on the SrcReg side.

## Root cause (source citations)

`llvm/lib/CodeGen/MachineCSE.cpp:173-223`:

```cpp
for (MachineOperand &MO : MI->all_uses()) {
  Register Reg = MO.getReg();
  ...
  MachineInstr *DefMI = MRI->getVRegDef(Reg);
  if (!DefMI || !DefMI->isCopy())
    continue;
  Register SrcReg = DefMI->getOperand(1).getReg();
  ...
  if (!MRI->constrainRegAttrs(SrcReg, Reg))
    continue;                                     // <-- side effects already done
  ...
  MO.setReg(SrcReg);                              // <-- only does this on success
  MRI->clearKillFlags(SrcReg);
  if (OnlyOneUse) {
    DefMI->changeDebugValuesDefReg(SrcReg);
    DefMI->eraseFromParent();
  }
  Changed = true;
}
```

`llvm/lib/CodeGen/MachineRegisterInfo.cpp:90-117` -- `constrainRegAttrs` is
mutating: it calls `setRegClassOrRegBank(Reg, ...)` and may call the mutating
`constrainRegClass(*this, Reg, ...)`.

So when the loop processes use #0 (Reg_0, SrcReg_0) successfully, it both
narrows `SrcReg_0` and rewrites `MO_0` to use `SrcReg_0`. When it processes
use #1 (Reg_1, SrcReg_1) and `constrainRegAttrs` succeeds early but later
fails (e.g. because the LLT didn't match), `SrcReg_1`'s reg-class has been
narrowed -- but we `continue` and never rewrite `MO_1`. `SrcReg_1` is still
used by its original COPY (which is still alive because `OnlyOneUse` was
false for it, or because we abandoned the rewrite). That original COPY is
defined in terms of the unnarrowed source, and other uses of `SrcReg_1`
elsewhere in the function now operate under the narrower class.

## What can go wrong
Narrowing alone is not a miscompile -- the narrower class is a subset of the
wider class, so existing instructions still encode legally. The downstream
effect is regalloc seeing a more constrained allocation set and possibly
introducing extra copies or spills to bridge the narrower SrcReg back to a
wider use. With unfortunate timing this can cascade into a register
allocation failure (out-of-physregs) in tight functions, which then
manifests as an InlineSpiller assertion (related to w348-style issues).

It can also confuse later passes that consult `MRI->getRegClass(SrcReg)` for
legality (e.g. `TII->reMaterialize`, `TII->isSafeToMoveRegClassDefs`) and
make them give a different answer than they would have if MachineCSE had
not run.

## Reproducer sketch
Build a function where MI has two vreg uses:
- use #0: vreg `%0` defined by `COPY %wide:gr64` -- successful constrain
  (`%0:gr32` → constrained against `%wide:gr64`'s subreg/class). `SrcReg_0`
  (which is the COPY's source) gets its class narrowed.
- use #1: vreg `%1` defined by `COPY %something:fr32` with a subreg --
  the `if (DefMI->getOperand(1).getSubReg()) continue;` at line 199-200
  catches subreg copies first. Or instead, contrive an LLT mismatch
  (`MRI->getType(SrcReg).isValid() && getType(Reg).isValid() && != `) so
  that `constrainRegAttrs` returns false in `MachineRegisterInfo.cpp:95-97`.

The minimal MIR test would need GISel-style typed vregs to trigger the LLT
path. On the X86 pre-RA pipeline it can trigger via the reg-class
intersection path: a vreg whose `getRegClassOrRegBank` returns a class that
intersects to the empty set with `Reg`'s class via the constrainRegClass call
at line 107-110.

## Fix sketch
Either:
1. Snapshot `(getRegClass(SrcReg), getType(SrcReg))` before the call,
   and restore on `continue`; or
2. Do the constrain check side-effect-free first (mirror the predicate of
   `constrainRegAttrs` into a `canConstrainRegAttrs` helper); call the
   mutating version only when we are committed to rewriting.

The same fix template applies to w342 (`ProcessBlockCSE`'s own
`constrainRegAttrs` loop), and they should probably be fixed together by
giving `MachineRegisterInfo` a non-mutating
`canConstrainRegAttrs(Reg, ConstrainingReg)` to predicate-check before
mutation.

## Cite
- `llvm/lib/CodeGen/MachineCSE.cpp:173-223` `PerformTrivialCopyPropagation`
- `llvm/lib/CodeGen/MachineRegisterInfo.cpp:90-117` `constrainRegAttrs`
- `llvm/lib/CodeGen/MachineRegisterInfo.cpp:60-87` `constrainRegClass`
  (the helper that mutates RegClass on intersection)
