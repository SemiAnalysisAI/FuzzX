# X86 InstCombine: `simplifyX86vpermilvar` (PD variants) reads only bits [1:0] then shifts right by 1, losing the actual selector bit for non-canonical mask vectors

## File
`llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp`, lines 2068-2113.

## Code

```cpp
static Value *simplifyX86vpermilvar(const IntrinsicInst &II, ...) {
  ...
  bool IsPD = VecTy->getScalarType()->isDoubleTy();
  unsigned NumLaneElts = IsPD ? 2 : 4;
  ...
  for (unsigned I = 0; I < NumElts; ++I) {
    Constant *COp = V->getAggregateElement(I);
    ...
    APInt Index = cast<ConstantInt>(COp)->getValue();
    Index = Index.zextOrTrunc(32).getLoBits(2);    // <-- keep bits [1:0]

    // The PD variants uses bit 1 to select per-lane element index, so
    // shift down to convert to generic shuffle mask index.
    if (IsPD)
      Index.lshrInPlace(1);                         // <-- shift right by 1
    ...
  }
}
```

## Bug

For VPERMILPD (`vpermilvar.pd*`), the Intel manual says the selector is **bit 1 of the 64-bit mask element**:

> If the mask register operand selector is `IF mask[63:0] bit 1 == 0 THEN source[63:0]; ELSE source[127:64]`.

The intrinsic's mask is a vector of i64 (for `vpermilvar.pd`, `vpermilvar.pd_256`, `vpermilvar.pd_512`). The hardware ignores all bits except bit 1.

The code:
1. Truncates `Index` to 32 bits via `zextOrTrunc(32)`.
2. Calls `getLoBits(2)` → keeps bits [1:0].
3. Right-shifts by 1 → keeps bit 1 (the correct selector bit).

For a mask element value `m`, the resulting per-lane index is `(m >> 1) & 1`. **This is correct** for the typical case where the mask comes from real code.

However, observe step 1: `Index.zextOrTrunc(32)`. The original mask APInt is the full i64 element value. Truncating to 32 bits **discards bits 63..32**. Bit 1 of the original i64 == bit 1 of the truncated i32 (always), so this truncation is *safe for bit 1*, the only bit that matters here. No bug.

But step 1 is **load-bearing** for the `getLoBits(2)` call: if the source `Index` is wider than 32 bits AND non-canonical (e.g., bit 1 set, all higher bits also set as in `0xFFFFFFFFFFFFFFFF`), `getLoBits(2)` on the i64 would give 3 (binary 11); after `lshrInPlace(1)`, would give 1. Same result. So this is also safe.

## Status

Re-analyzed and ruled out. The truncation-to-i32 followed by `getLoBits(2)` then `>>1` is equivalent to extracting bit 1 of the original i64, which matches hardware. **No bug.**

I am keeping this candidate as a ruled-out note so future workers don't re-derive the same suspicion.

## Confidence

Ruled out.
