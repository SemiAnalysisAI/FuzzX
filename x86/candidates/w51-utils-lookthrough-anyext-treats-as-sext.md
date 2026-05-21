# w51: Utils getConstantVRegValWithLookThrough treats G_ANYEXT as G_SEXT when reconstructing constant

File: `llvm/lib/CodeGen/GlobalISel/Utils.cpp`
Function: `getConstantVRegValWithLookThrough` (lines 333-391)

## Bug

When `LookThroughAnyExt` is true, the walker records the bridging
opcode (line 345-356):

```cpp
case TargetOpcode::G_ANYEXT:
  if (!LookThroughAnyExt)
    return std::nullopt;
  [[fallthrough]];
case TargetOpcode::G_TRUNC:
case TargetOpcode::G_SEXT:
case TargetOpcode::G_ZEXT:
  SeenOpcodes.push_back({MI->getOpcode(), ...});
  VReg = MI->getOperand(1).getReg();
  break;
```

Then on the way back up (lines 375-388), the reconstruction picks an
extension type for each opcode:

```cpp
for (auto &Pair : reverse(SeenOpcodes)) {
  switch (Pair.first) {
  case TargetOpcode::G_TRUNC: Val = Val.trunc(Pair.second); break;
  case TargetOpcode::G_ANYEXT:
  case TargetOpcode::G_SEXT:  Val = Val.sext(Pair.second); break;
  case TargetOpcode::G_ZEXT:  Val = Val.zext(Pair.second); break;
  }
}
```

`G_ANYEXT` is silently treated as `G_SEXT`. ANYEXT semantically picks
**any** value for the upper bits; a caller asking "what concrete value
does this register have?" via `LookThroughAnyExt=true` is told a value
that is one valid choice — but **other downstream consumers of the
same anyext result may have assumed zero-extension** (e.g., a peephole
that later mask-ANDs the upper bits to zero on the assumption that
ANYEXT-to-zero is poison-free).

The bug is that two different callers can get a different concrete
APInt from the same %vreg depending on which extension the lookthrough
walker picked. Constant-folding using this value can then materialize a
value the original IR never had.

## Concrete example

```
%c   = G_CONSTANT i8 -1            ; 0xFF
%ext = G_ANYEXT i32 %c             ; upper bits are "any"
%y   = G_AND   i32 %ext, 0xFFFFFF00 ; folder wants to fold to a constant
```

If a combiner calls `getAnyConstantVRegValWithLookThrough(%ext, MRI,
LookThroughInstrs=true, LookThroughAnyExt=true)`, it gets back
APInt(32, 0xFFFFFFFF) (sext). Folding `%y = AND 0xFFFFFFFF, 0xFFFFFF00`
yields 0xFFFFFF00. But selection might lower `%ext` as a plain
`MOVZX` (zero-extend), giving 0x000000FF in the physical register, and
then `%y = AND 0x000000FF, 0xFFFFFF00 = 0`. The constant fold and the
actual codegen disagree.

## Impact

Latent across all targets that consume `getAnyConstantVRegValWithLookThrough(
..., LookThroughAnyExt=true)`. Searching shows AArch64 GISel combiner
and AMDGPU regbankselect both call this with `LookThroughAnyExt=true`.
The x86 GISel path uses it indirectly through CombinerHelper templates.

## Fix

Either:
1. Return `std::nullopt` when an ANYEXT was traversed AND the upper
   bits would be observed (i.e. the next op up is a use of the upper
   bits), or
2. Have the caller pass an explicit policy ("treat anyext as zext" /
   "treat anyext as sext" / "fail on anyext") instead of silently
   choosing sext.

## Status

Source-confirmed; concrete miscompile would require a CombinerHelper
pattern that folds via `LookThroughAnyExt` and another pattern that
lowers the same ANYEXT to MOVZX. No reproducer attempted.
