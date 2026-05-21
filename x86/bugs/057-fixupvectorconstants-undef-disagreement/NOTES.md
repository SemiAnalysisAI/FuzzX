# X86FixupVectorConstants: rebuildExtCst silently treats undef vector elements as 0, but the original load with `undef` lanes could have been the masked side of a later bitop where the rewritten zero load freezes the result

## File / lines
`llvm/lib/Target/X86/X86FixupVectorConstants.cpp`, line 92-96 (the top-level
`extractConstantBits(C)` mapping `isa<UndefValue>(C) -> APInt::getZero(NumBits)`),
and lines 306-335 (`rebuildExtCst`).

```cpp
static std::optional<APInt> extractConstantBits(const Constant *C) {
  unsigned NumBits = C->getType()->getPrimitiveSizeInBits();
  if (isa<UndefValue>(C))
    return APInt::getZero(NumBits);                // <-- undef freezes to 0
  ...
}
```
and inside `rebuildExtCst`:
```cpp
APInt TruncBits = APInt::getZero(NumElts * SrcEltBitWidth);
for (unsigned I = 0; I != NumElts; ++I) {
  APInt Elt = Bits->extractBits(DstEltBitWidth, I * DstEltBitWidth);
  if ((IsSExt && Elt.getSignificantBits() > SrcEltBitWidth) ||
      (!IsSExt && Elt.getActiveBits() > SrcEltBitWidth))
    return nullptr;
  TruncBits.insertBits(Elt.trunc(SrcEltBitWidth), I * SrcEltBitWidth);
}
```

## Reasoning
The constant-pool load being rewritten reads the CP entry as plain bytes — for
undef elements there is no actual `undef` semantics at the machine level by the
time we reach this pass. However, `extractConstantBits` happily collapses any
`UndefValue` sub-Constant into bits=0 and then the rebuilder proves the
extension "fits" because 0 fits in 1 bit. The replacement load (e.g.
`VPMOVSXBQrm`) then reads a freshly-built CP entry where that lane is now
*concretely* 0, then **sign-extends 0 to 0**. For zero-extend (`VPMOVZXBQrm`)
the same thing happens.

The two domains of concern:

1. `rebuildExtCst`'s loop guard treats `Elt = 0` as `getSignificantBits() == 0`
   (or `getActiveBits() == 0`), so it freely accepts the undef lane regardless
   of what the surrounding lanes need. Combined with the fact that the
   `rebuildConstant` step writes 0 into the CP, the rewritten instruction
   sign-extends/zero-extends 0. This is a *refinement* in pure value semantics
   for an isolated undef lane (allowed), BUT…

2. `extractConstantBits` for **whole-vector undef** returns all-zero. A CP entry
   that ConstantFolding chose to leave as a poison/undef pattern (e.g. from a
   masked load) is now replaced with `vpbroadcastb $0` / `vpmov*x $0` — i.e.
   the post-pass produces *deterministic zero where the IR allowed any value*.
   That is itself fine (zero refines undef), but it changes downstream
   instruction-equivalence: e.g. a later `vpxor %ymm0, %ymm0, %ymm0` that the
   scheduler/peephole intended to fold with the loaded undef constant no longer
   sees an "anything" lane, and other fixup logic (especially the
   broadcast-fold table lookup at line 705) may make different choices on the
   first run vs. a re-run, breaking idempotency of the pass.

The deeper risk is the *combination* with `getSplatableConstant`'s undef-aware
path (line 176-209). That path correctly tolerates `<i32 7, undef, i32 7,
undef>` as a 32-bit splat of 7. But `extractConstantBits` for the same vector
returns `<7, 0, 7, 0>` — not a 32-bit splat. The two helpers disagree on the
meaning of undef. `FixupConstant` walks the Fixup list and tries each rebuilder;
which rule wins depends on table ordering. For pre-AVX2 `VMOVDQUrm`, the table
puts `VPMOVSXBQrm` before `VBROADCASTSSrm` (line 583-587). With a CP like
`<i32 7, undef, i32 7, undef>`:
   * `rebuildSExtCst(...,2,8)` extracts bits `<7,0,7,0>` (128 bits), takes lane
     0 (DstEltBitWidth=64) = 0x0000000700000000\__decimal=30064771072__\, has 35
     significant bits → does not fit in 8 → returns nullptr.
   * Same for ZExt. Falls through to broadcast and matches.

OK in this case, but reorder the constant to `<i32 7, undef>` (only 64 bits in
a 64-bit splat sense for `VBROADCASTSDrm`) and the disagreement bites:
`getSplatableConstant(C,32)` returns 7; `extractConstantBits(C)` returns
`<7,0>` and `isSplat(32)` is false at the early-return at line 171, so we fall
into the undef-aware path. Two different helpers reach different conclusions
about whether `<i32 -127, undef>` is a *valid sign-extendable* 8-bit splat.

## Candidate IR
```llvm
target triple = "x86_64-unknown-linux-gnu"
; -mattr=+avx,-avx2  (forces the no-AVX2 broadcast path)

define <8 x i32> @blkid() {
  ; CP constant has an undef lane on purpose
  ret <8 x i32> <i32 -127, i32 undef, i32 -127, i32 undef,
                 i32 -127, i32 undef, i32 -127, i32 undef>
}
```
Run with `llc -mtriple=x86_64-- -mattr=+avx,-avx2 -O2 test.ll -o -`.

## Wrong outcome
Comparison of two near-identical IRs with the only difference being the
undef-lane value (e.g. switching `undef` to `i32 -127`) produces *different*
instruction selection in this pass — non-monotone w.r.t. undef refinement.
Replacement load `vpmovsxbq` with src-byte 0 vs. byte `-127`. The two
behaviors are individually legal, but the inconsistency means downstream
instruction-equality assumptions (e.g. coalescing of identical CP entries,
machine-CSE between two basic blocks that differ only by which lanes the
optimizer marked undef) fail, and the pass is not idempotent against
re-running the constant-fold/strict-undef pipeline.
