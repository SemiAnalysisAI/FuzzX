# w353: X86 optimizeCompareInstr SignedMin check fails for sign-extended int64 CmpValue

## Severity
Latent miscompile. Reachable only when (a) `analyzeCompare` returns a sign-extended `int64_t` `CmpValue` for a sub-64-bit compare (CMP8ri/CMP16ri/CMP32ri/SUB*ri) whose immediate equals the bit-width's `SignedMin`, and (b) an adjacent earlier CMP/SUB sets `ImmDelta = 1` (so OIValue = CmpValue - 1, which underflowed in the sub-64-bit width). Constant-folding in DAGCombine swallows the obvious IR-level patterns, but the bug is reachable through MIR-level constructions (e.g., LICM/MachineCSE hoisting one CMP across another, custom MIR passes that synthesize comparisons against `INT_MIN`).

## Suspicious code
`llvm/lib/Target/X86/X86InstrInfo.cpp:5559-5598` — `optimizeCompareInstr` reading `analyzeCompare`'s `CmpValue`:

```cpp
unsigned BitWidth = RI.getRegSizeInBits(*MRI->getRegClass(SrcReg));
switch (OldCC) {
case X86::COND_L: // x <s (C + 1)  -->  x <=s C
  if (ImmDelta != 1 || APInt::getSignedMinValue(BitWidth) == CmpValue)
    return false;                                             // 5561
  ReplacementCC = X86::COND_LE;
  break;
case X86::COND_GE: // x >=s (C + 1)  -->  x >s C
  if (ImmDelta != 1 || APInt::getSignedMinValue(BitWidth) == CmpValue)
    return false;                                             // 5570
  ReplacementCC = X86::COND_G;
  break;
case X86::COND_G: // x >s (C - 1)  -->  x >=s C
  if (ImmDelta != -1 || APInt::getSignedMaxValue(BitWidth) == CmpValue)
    return false;                                             // 5580
  ReplacementCC = X86::COND_GE;
  break;
case X86::COND_LE: // x <=s (C - 1)  -->  x <s C
  if (ImmDelta != -1 || APInt::getSignedMaxValue(BitWidth) == CmpValue)
    return false;                                             // 5590
  ReplacementCC = X86::COND_L;
  break;
```

`CmpValue` is `int64_t`. For sub-64-bit compares, `analyzeCompare` returns the raw `MachineOperand::getImm()` (e.g., `X86InstrInfo.cpp:4840`), which can be a sign-extended negative `int64_t` for negative immediates (`-128` for CMP8ri, `-2147483648` for CMP32ri, etc.).

`APInt::getSignedMinValue(BitWidth) == CmpValue` resolves to `APInt::operator==(uint64_t)` (defined at `include/llvm/ADT/APInt.h:1076`):

```cpp
bool operator==(uint64_t Val) const {
  return (isSingleWord() || getActiveBits() <= 64) && getZExtValue() == Val;
}
```

For `BitWidth=32`:
- LHS: `APInt::getSignedMinValue(32)` has `getZExtValue() == 0x80000000`.
- RHS: `CmpValue = -2147483648` is implicitly cast to `uint64_t` as `0xFFFFFFFF'80000000`.
- `0x80000000 == 0xFFFFFFFF80000000` → **false**.

So the SignedMin guard silently fails. The optimization proceeds, and the rewritten condition (e.g., `COND_LE` instead of `COND_L`) reads "x ≤s OIValue" where OIValue = CmpValue - 1. But OIValue was stored in the earlier SUB/CMP's `int64_t` operand without further width-truncation in the MIR; its semantic value in BitWidth has **underflowed** (`INT_MIN - 1` wraps to `INT_MAX`). The transformation `x <s INT_MIN` (always false) becomes `x ≤s INT_MAX` (always true) — a complete sign flip.

The same analysis applies symmetrically for `COND_GE` at line 5570, and for `COND_G`/`COND_LE` at lines 5580/5590 (`SignedMaxValue` vs. CmpValue, where CmpValue could be the positive sign-extended representation of `SignedMax`; in that direction it usually matches because positive values are not sign-extended differently, but the principle of the comparison is fragile).

## Probe IR (does not currently trigger via IR — DAGCombine folds `icmp slt x, INT_MIN`):

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(ptr %p) {
  %x = load volatile i32, ptr %p
  %c1 = icmp slt i32 %x, -2147483647
  %c2 = icmp slt i32 %x, -2147483648
  %s1 = zext i1 %c1 to i32
  %s2 = zext i1 %c2 to i32
  %r = add i32 %s1, %s2
  ret i32 %r
}
```

`llc -O2` collapses both compares to a single `negl + seto`, never reaching the buggy path through PeepholeOptimizer → `optimizeCompareInstr`. The bug is latent against future MIR-level transformations.

## Fix sketch
Compare on the truncated value, not the raw `int64_t`:
```cpp
APInt CmpVal(BitWidth, CmpValue, /*isSigned=*/true);
if (ImmDelta != 1 || APInt::getSignedMinValue(BitWidth) == CmpVal)
  return false;
```
or equivalently:
```cpp
if (ImmDelta != 1 ||
    APInt::getSignedMinValue(BitWidth).getSExtValue() == CmpValue)
  return false;
```
