# m101: `performAddCarrySubCarryCombine` rewrites carry-out of `UADDO_CARRY((add x,y), 0, cc)` incorrectly

*Discovery method: code inspection.* (Re-audit of integer-arithmetic
combiners.)

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:17527-17533`:

```cpp
// uaddo_carry (add x, y), 0, cc => uaddo_carry x, y, cc
// usubo_carry (sub x, y), 0, cc => usubo_carry x, y, cc
unsigned LHSOpc = LHS.getOpcode();
unsigned Opc = N->getOpcode();
if ((LHSOpc == ISD::ADD && Opc == ISD::UADDO_CARRY) ||
    (LHSOpc == ISD::SUB && Opc == ISD::USUBO_CARRY)) {
  SDValue Args[] = {LHS.getOperand(0), LHS.getOperand(1), N->getOperand(2)};
  return DAG.getNode(Opc, SDLoc(N), N->getVTList(), Args);
}
```

The fold replaces `UADDO_CARRY((x+y) mod 2^32, 0, cc)` with
`UADDO_CARRY(x, y, cc)` reusing `N->getVTList()`, so DAGCombiner's
`CombineTo` rewires both results -- value AND carry-out.

The two carry-outs are not equal:

| node | carry-out |
| --- | --- |
| original | `((x+y) mod 2^32) + cc >= 2^32` |
| folded   | `x + y + cc >= 2^32`            |

The fold is value-correct (both produce `(x+y+cc) mod 2^32`) but
**carry-out-incorrect** whenever `x+y` wraps in i32.

Example: `x=0xFFFFFFFF, y=1, cc=0`:
* original carry-out = 0  (`0 + 0 < 2^32`)
* folded carry-out   = 1  (`0x100000000 >= 2^32`)

The combiner does NOT check `LHS.hasOneUse()` and does NOT check that
`N`'s carry-out is dead.  If the outer UADDO_CARRY's carry-out has any
consumer, that consumer receives the wrong bit.

The symmetric `USUBO_CARRY((x-y), 0, cc) -> USUBO_CARRY(x, y, cc)` has
the same flaw on the borrow output.

## Contrast: the generic DAGCombiner version is safe

`lib/CodeGen/SelectionDAG/DAGCombiner.cpp:3513-3517` has a sibling
rewrite that *is* safe because the outer is a plain `add`:

```cpp
// (add X, (uaddo_carry Y, 0, Carry)) -> (uaddo_carry X, Y, Carry)
if (N1.getOpcode() == ISD::UADDO_CARRY && isNullConstant(N1.getOperand(1)) &&
    N1.getResNo() == 0)
  return DAG.getNode(ISD::UADDO_CARRY, DL, N1->getVTList(),
                     N0, N1.getOperand(0), N1.getOperand(2));
```

The new multi-result node's carry-out is a brand-new SDValue; the
inner `N1`'s old carry-out users keep pointing at the unmodified old
`N1`.  The AMDGPU target combiner cannot use the same shape because
its outer node *is* a multi-result `UADDO_CARRY` and `CombineTo`
rewires *both* outputs.

## How the buggy shape arises

Source IR: `uaddo(add(x,y), zext(cc))` (any pattern that adds a
carry-shaped i1 onto a plain wrapping add).

1. Generic `visitUADDOLike` (DAGCombiner.cpp:3649) folds
   `uaddo X, Carry -> uaddo_carry X, 0, Carry`, producing
   `uaddo_carry(add(x,y), 0, cc)`.
2. Target `performAddCarrySubCarryCombine` (SIISelLowering.cpp:17527)
   rewrites that to `uaddo_carry(x, y, cc)`, silently changing the
   carry-out semantics.

## Asm-level demonstration (gfx950)

`llc -mcpu=gfx950 reduced.ll`:

O0:
```
v_add_u32_e64 v1, v1, v2              ; xy = x + y (no carry tracked)
v_add_co_u32_e64 v2, s[2:3], v1, v2   ; xy + cc, carry from (xy mod 2^32) + cc
v_cndmask_b32_e64 v1, 0, 1, s[2:3]
```

O2:
```
v_add_co_u32_e32 v0, vcc, v0, v4      ; z + w  -> cc in vcc
v_addc_co_u32_e32 v1, vcc, v1, v2, vcc; v1 = x + y + cc, vcc = full-precision carry
v_cndmask_b32_e64 v2, 0, 1, vcc       ; *** carry now reflects (x+y+cc) >= 2^32 ***
```

With `x=0xFFFFFFFF, y=1, z=0, w=0`:
* O0 stores `co_i32 = 0` (correct per IR).
* O2 stores `co_i32 = 1`.

## Suggested fix

Gate the rewrite on `N->hasAnyUseOfValue(1) == false`:

```cpp
if ((LHSOpc == ISD::ADD && Opc == ISD::UADDO_CARRY) ||
    (LHSOpc == ISD::SUB && Opc == ISD::USUBO_CARRY)) {
  if (N->hasAnyUseOfValue(1))   // carry-out is observable -> bail
    return SDValue();
  ...
}
```

Or require both `LHS.hasOneUse()` and `N`'s carry-out unused.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces -- O2 emits `v_addc_co_u32` whose carry differs from the IR semantics. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present (combine in tree). |

Not a HEAD-only regression -- `performAddCarrySubCarryCombine` has
been in `SIISelLowering.cpp` for some time.
