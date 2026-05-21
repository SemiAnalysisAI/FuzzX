# InstCombine narrowInsElt drops zext nneg flag when sinking insertelement

File: llvm/lib/Transforms/InstCombine/InstCombineVectorOps.cpp:1623-1652

## Reasoning

`narrowInsElt` recognises the pattern:

```
inselt (cast X), (cast Y), Index
```

(where the cast is one of FPExt / SExt / ZExt) and rewrites it as a single cast
applied to a smaller insertelement:

```
inselt (cast X), (cast Y), Index  -->  cast (inselt X, Y, Index)
```

The implementation:

```cpp
Value *NewInsElt = Builder.CreateInsertElement(X, Y, InsElt.getOperand(2));
return CastInst::Create(CastOpcode, NewInsElt, InsElt.getType());
```

`CastInst::Create` creates a fresh `ZExt`/`SExt`/`FPExt` with default flags. It
does NOT propagate flags from the original cast operands. For `ZExt`, this
means the `nneg` flag is silently dropped even when BOTH the vector-cast
operand and the scalar-cast operand had `nneg` set. In that case, every lane of
the input to the merged `zext` is known non-negative, so the merged `zext`
could (and should) inherit `nneg`. Dropping it forfeits subsequent optimisation
opportunities (e.g. `sext`/`zext` collapsing, `lshr`-based simplifications,
range derivations).

The same pattern applies to other cast flags that might be added in future
(e.g. `nuw`/`nsw` on `trunc`); `narrowInsElt` will silently drop those too.

This is a missed-optimisation, not a miscompile — dropping `nneg` is a
refinement.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <4 x i32> @test_zext_nneg(<4 x i8> %x, i8 %y) {
  %vx = zext nneg <4 x i8> %x to <4 x i32>
  %sy = zext nneg i8 %y to i32
  %r  = insertelement <4 x i32> %vx, i32 %sy, i64 2
  ret <4 x i32> %r
}
```

`opt -passes=instcombine -S` output:

```
define <4 x i32> @test_zext_nneg(<4 x i8> %x, i8 %y) {
  %1 = insertelement <4 x i8> %x, i8 %y, i64 2
  %r = zext <4 x i8> %1 to <4 x i32>     ; <-- nneg dropped
  ret <4 x i32> %r
}
```

The `nneg` flag is dropped on the final `zext`. The expected output is
`%r = zext nneg <4 x i8> %1 to <4 x i32>` because all lanes of `%1` are
non-negative (`nneg` was asserted on both operands of the original
insertelement).

## Fix sketch

```cpp
auto *Cast = CastInst::Create(CastOpcode, NewInsElt, InsElt.getType());
// Intersect flags from the two original casts; both casts are required
// to match (line 1636-1641 already enforces same opcode).
if (auto *VecCast = cast<CastInst>(Vec)) {
  Cast->copyIRFlags(VecCast);
  if (auto *ScCast = dyn_cast<CastInst>(Scalar))
    Cast->andIRFlags(ScCast);
}
return Cast;
```

`copyIRFlags`/`andIRFlags` already handle `nneg` (`PossiblyNonNegInst`) and
`trunc nuw/nsw` correctly (Instruction.cpp:721-808), so adding the call is the
whole fix.
