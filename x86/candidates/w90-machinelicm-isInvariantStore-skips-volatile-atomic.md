# MachineLICM `isInvariantStore` does not check volatile/atomic MMO bits

File: `llvm/lib/CodeGen/MachineLICM.cpp:1013-1041` (`isInvariantStore`)
Gating call site: `llvm/lib/CodeGen/MachineLICM.cpp:1078-1085`
(`MachineLICMImpl::IsLICMCandidate`).

## Bug

```cpp
static bool isInvariantStore(const MachineInstr &MI,
                             const TargetRegisterInfo *TRI,
                             const MachineRegisterInfo *MRI) {
  bool FoundCallerPresReg = false;
  if (!MI.mayStore() || MI.hasUnmodeledSideEffects() ||
      (MI.getNumOperands() == 0))
    return false;
  // ...only checks every operand is an imm or a caller-preserved physreg.
  return FoundCallerPresReg;
}
```

There are two filters missing:
1. `MI.isVolatile()` / `MI.hasOrderedMemoryRef()` — a volatile or atomic store
   is not safe to hoist regardless of address invariance.
2. The atomic-ordering flag on the MMO (an unordered atomic is movable; a
   monotonic-or-stronger atomic is not).

`IsLICMCandidate` then does:
```cpp
if ((!I.isSafeToMove(DontMoveAcrossStore)) &&
    !(HoistConstStores && isInvariantStore(I, TRI, MRI)))
  return false;
```

with the cl::opt `HoistConstStores` defaulting to **true**, so any store that
satisfies `isInvariantStore` bypasses `isSafeToMove` entirely — exactly the
function that **does** check `hasOrderedMemoryRef`.

## Why it matters on x86

`isCallerPreservedPhysReg` returns true for registers the function will preserve
across calls. On x86_64 this includes the frame/base pointer and segment-relative
addresses like `%fs`-based TLS slots when accessed via a TLS sequence whose
final lea uses a callee-saved register. So a volatile store like

```
MOV32mi %rbp, 1, $noreg, -8, $noreg, 42 :: (volatile store (s32) into %ir.cookie)
```

inside a loop body where `%rbp` is the frame pointer would satisfy
`isInvariantStore` (only an imm operand and a caller-preserved physreg), and
the volatile-store guard in `isSafeToMove` is bypassed.

## Repro shape (MIR via run-pass)

```mir
---
name: f
tracksRegLiveness: true
body: |
  bb.0:
    successors: %bb.1
    JMP_1 %bb.1
  bb.1:
    successors: %bb.1, %bb.2
    MOV32mi $rbp, 1, $noreg, -8, $noreg, 42 :: (volatile store (s32) into %ir.cookie)
    JCC_1 %bb.1, 4, implicit $eflags
  bb.2:
    RET 0
...
```

```
llc -mtriple=x86_64-linux-gnu -run-pass=machinelicm -hoist-const-stores=true \
    in.mir -o -
```

Expected: the volatile store remains inside the loop body.
Buggy: the volatile store is hoisted to a preheader (or sunk above the loop
edge), changing the observable count of writes performed by the function.

## Fix

Add at the top of `isInvariantStore`:
```cpp
if (MI.hasOrderedMemoryRef())
  return false;
for (const MachineMemOperand *MMO : MI.memoperands())
  if (MMO->isVolatile() || !MMO->isUnordered())
    return false;
```
(The `hasOrderedMemoryRef` check already encompasses volatile-loads-as-stores
and ordered atomic stores; the explicit MMO walk is belt-and-braces for the
case where `mayStore()` is true but the MMO list is empty — in which case
`hasOrderedMemoryRef` returns true via the `memoperands_empty()` branch and
the function should also bail.)

## Status

The function-level audit shows the latent gap. Triggering from regular IR is
constrained by the `isCallerPreservedPhysReg` requirement on every operand,
so end-to-end miscompiles need either (a) a hand-written MIR test driving
`-run-pass=machinelicm`, or (b) a target hook that promotes a frame-pointer-
relative store into the `isCallerPreservedPhysReg` set. The code path itself
is unambiguously incorrect.
