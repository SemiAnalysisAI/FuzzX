# optimizeCompareInstr ImmDelta guard fails for narrow widths due to unsigned APInt compare

File: llvm/lib/Target/X86/X86InstrInfo.cpp:5554-5601

## Description
In `optimizeCompareInstr`, when an earlier CMP/SUB has imm `OIValue`
and a later CMP has `CmpValue = OIValue ± 1`, the pass rewrites
condition codes (`x <s C+1` -> `x <=s C`, etc.). The guards that
forbid the rewrite at the signed extremes use APInt equality:

```cpp
unsigned BitWidth = RI.getRegSizeInBits(*MRI->getRegClass(SrcReg));
switch (OldCC) {
case X86::COND_L: // x <s (C+1) -> x <=s C
  if (ImmDelta != 1 || APInt::getSignedMinValue(BitWidth) == CmpValue)
    return false;
  ReplacementCC = X86::COND_LE;
  break;
...
case X86::COND_LE: // x <=s (C-1) -> x <s C
  if (ImmDelta != -1 || APInt::getSignedMaxValue(BitWidth) == CmpValue)
    return false;
  ReplacementCC = X86::COND_L;
  break;
```

`APInt::operator==(uint64_t)` (APInt.h:1076) compares via
`getZExtValue() == Val` where `Val = (uint64_t)CmpValue`. For
sub-64-bit widths (CMP8ri / CMP16ri / CMP32ri), `CmpValue` is a
sign-extended `int64_t`, while `getSignedMinValue(BitWidth)` /
`getSignedMaxValue(BitWidth)` return APInts that zero-extend to their
*positive* representation. E.g. for `CMP8ri %al, -128`:

  CmpValue          = 0xFFFFFFFFFFFFFF80  (int64_t -128)
  getSignedMinValue(8).getZExtValue() = 128 = 0x80

The guard `== CmpValue` is false even though semantically CmpValue
*is* SignedMin(8). The transformation then proceeds in a case it was
specifically designed to reject. For COND_L the rewrite becomes
`x <=s C` where `C = SignedMin - 1` underflows the width; for COND_GE
the rewrite produces `x >s C` symmetrically wrong; for COND_LE/COND_G
the symmetric mirror at SignedMax misfires for CMP*ri with
`CmpValue = 0x7F` (s8) stored as 127, where OIValue = 128 must be
stored as -128 due to sign-extension, making `ImmDelta = -1 - 256`
mod arith which itself fails `ImmDelta != -1`. The dangerous combo is
the SignedMin guard.

Concretely the cmp pair to reproduce:
```
  CMP8ri %al, -127   ; OIValue = -127
  ... no clobber of EFLAGS ...
  CMP8ri %al, -128   ; CmpValue = -128, ImmDelta = -1
  JCC .LBB, COND_LE  ; x <=s -128 -> always-equals branch
```
The guard wants to bail because `CmpValue == SignedMin(8)`, but the
APInt equality returns false (128 != 0xFF..F80). The rewrite proceeds
to `JCC COND_L` against the OI flags `cmp %al, -127`, i.e. `x <s -127`.
Both predicates have the same set of inputs that satisfy them only at
x = -128; at x = -129, -130 (truncated to 8 bits the values that
sign-extend below -128, e.g. wrap-around register reads) the rewritten
predicate diverges from the original. More importantly the precondition
that protected against representation-edge wrap was bypassed.

## Wrong outcome
For 8/16/32-bit CMPri at SignedMin or SignedMax with ImmDelta = ±1, the
rewrite that the guard was supposed to block is allowed, producing wrong
condition codes for users of the *removed* CMP's EFLAGS. The 64-bit case
is unaffected because the APInt and the int64 representations coincide.

## Reproducer (MIR sketch)
```
# llc -run-pass=peephole-opt -mtriple=x86_64-- repro.mir
---
name: f
body: |
  bb.0:
    liveins: $al
    CMP8ri $al, -127, implicit-def $eflags
    JCC_1 %bb.2, 14, implicit $eflags
    CMP8ri $al, -128, implicit-def $eflags  ; gets erased
    JCC_1 %bb.2, 14, implicit $eflags       ; COND_LE -> rewritten to COND_L
  bb.1:
    RET64
  bb.2:
    RET64
...
```
After peephole: the second CMP is removed and the second JCC's CC is
flipped to COND_L using the first CMP's flags, but at the boundary
where the guard should have stopped us.

## Fix sketch
Replace
  `APInt::getSignedMinValue(BitWidth) == CmpValue`
with
  `APInt::getSignedMinValue(BitWidth).getSExtValue() == CmpValue`
or `(CmpValue & ((1LL<<BitWidth)-1)) == APInt::getSignedMinValue(BitWidth).getZExtValue()`.
