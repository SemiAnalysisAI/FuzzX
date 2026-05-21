# InstCombine narrowVectorSelect drops fast-math flags from sunk select

File: llvm/lib/Transforms/InstCombine/InstCombineVectorOps.cpp:2540-2571

## Reasoning

`narrowVectorSelect` matches a "widen then narrow back" select pattern:

```
shuf (sel (shuf NarrowCond, poison, WideMask), X, Y), poison, NarrowMask
  -->
sel NarrowCond, (shuf X, poison, NarrowMask), (shuf Y, poison, NarrowMask)
```

The final return creates the new `SelectInst`:

```cpp
return SelectInst::Create(NarrowCond, NarrowX, NarrowY);
```

The four-argument overload `SelectInst::Create(Cond, TV, FV)` does NOT
copy any IR flags from the original select. If the original wide select
carried fast-math flags (`nnan`, `ninf`, `nsz`, `arcp`, `contract`, `afn`,
`reassoc` — applicable when the operands are FP-typed), all of them are
silently dropped.

There is a six-argument overload
`SelectInst::Create(Cond, TV, FV, NameStr, InsertBefore, MDFrom)` that copies
debug location and named metadata from `MDFrom` via `Instruction::copyMetadata`,
but `copyMetadata` does NOT copy `SubclassOptionalData` (i.e. FMF). The fold
neither passes `MDFrom` nor calls `copyFastMathFlags`/`copyIRFlags`, so FMF is
unconditionally lost.

This is a missed-optimisation rather than a miscompile (dropping FMF is a
refinement), but it pessimises downstream FP-arithmetic combines that look
through this select.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <2 x float> @test(<2 x i1> %nc, <4 x float> %x, <4 x float> %y) {
  %wc  = shufflevector <2 x i1> %nc, <2 x i1> poison,
                       <4 x i32> <i32 0, i32 1, i32 poison, i32 poison>
  %sel = select nnan ninf nsz <4 x i1> %wc, <4 x float> %x, <4 x float> %y
  %r   = shufflevector <4 x float> %sel, <4 x float> poison,
                       <2 x i32> <i32 0, i32 1>
  ret <2 x float> %r
}
```

`opt -passes=instcombine -S` output:

```
define <2 x float> @test(<2 x i1> %nc, <4 x float> %x, <4 x float> %y) {
  %1 = shufflevector <4 x float> %x, <4 x float> poison, <2 x i32> <i32 0, i32 1>
  %2 = shufflevector <4 x float> %y, <4 x float> poison, <2 x i32> <i32 0, i32 1>
  %r = select <2 x i1> %nc, <2 x float> %1, <2 x float> %2   ; <-- nnan ninf nsz dropped
  ret <2 x float> %r
}
```

Expected: `%r = select nnan ninf nsz <2 x i1> %nc, ...` — the narrow select is
just a per-lane subset of the wide select, so any FMF that held for every lane
of the wide select also holds for every lane of the narrowed select.

## Fix sketch

Copy FMF (and metadata) from the original wide select explicitly:

```cpp
auto *OldSel = cast<SelectInst>(Shuf.getOperand(0));
auto *NewSel =
    SelectInst::Create(NarrowCond, NarrowX, NarrowY, "", nullptr, OldSel);
if (isa<FPMathOperator>(NewSel))
  NewSel->copyFastMathFlags(OldSel);
return NewSel;
```

The 6-arg constructor handles named metadata; the explicit
`copyFastMathFlags` covers FMF (which lives in `SubclassOptionalData`, not
metadata).
