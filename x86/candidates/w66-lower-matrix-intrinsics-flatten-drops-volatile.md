# w66: LowerMatrixIntrinsics fused dot-product FlattenArg drops volatile

## Root cause
`LowerMatrixIntrinsics::lowerDotProduct::FlattenArg` lowers a
`llvm.matrix.column.major.load` intrinsic to a plain `Builder.CreateLoad`
without propagating the i1 `isVolatile` operand (ArgIndex 2 of the intrinsic).

```
llvm/lib/Transforms/Scalar/LowerMatrixIntrinsics.cpp:1718-1726
auto FlattenArg = [&Builder, ...](Value *Op) {
  ...
  if (match(Op, m_Intrinsic<Intrinsic::matrix_column_major_load>(
                    m_Value(Arg)))) {
    auto *NewLoad = Builder.CreateLoad(Op->getType(), Arg);  // <-- no volatile
    Op->replaceAllUsesWith(NewLoad);
    eraseFromParentAndRemoveFromShapeMap(cast<Instruction>(Op));
    return;
  }
  ...
};
```

The `m_Intrinsic<matrix_column_major_load>(m_Value(), m_One())` precondition
on `CanBeFlattened` only forces stride==1; it does NOT constrain the
`isVolatile` arg. Therefore an intrinsic with `i1 true` for isVolatile is
matched and lowered to a plain `load`.

## Trigger condition (x86)
Dot product (1xN * Nx1) with `reassoc` fast-math flag.

```
target triple = "x86_64-unknown-linux-gnu"

define <1 x double> @matmul_vol(ptr %a, ptr %b) {
entry:
  %A = call <4 x double> @llvm.matrix.column.major.load.v4f64.i64(
        ptr %a, i64 1, i1 true,  i32 1, i32 4)   ; <-- volatile
  %B = call <4 x double> @llvm.matrix.column.major.load.v4f64.i64(
        ptr %b, i64 4, i1 false, i32 4, i32 1)
  %M = call reassoc <1 x double> @llvm.matrix.multiply.v1f64.v4f64.v4f64(
        <4 x double> %A, <4 x double> %B, i32 1, i32 4, i32 1)
  ret <1 x double> %M
}
```

After `opt -passes=lower-matrix-intrinsics -S`:

```
define <1 x double> @matmul_vol(ptr %a, ptr %b) {
entry:
  %col.load = load <4 x double>, ptr %b, align 8        ; <-- non-volatile (originally non-volatile B - OK)
  %0        = load <4 x double>, ptr %a, align 32       ; <-- non-volatile, but A WAS volatile
  %1 = fmul <4 x double> %0, %col.load
  %2 = call reassoc double @llvm.vector.reduce.fadd.v4f64(double 0.000000e+00, <4 x double> %1)
  %3 = insertelement <1 x double> poison, double %2, i64 0
  ret <1 x double> %3
}
```

`%A` was created via `matrix.column.major.load(..., i1 true, ...)` (volatile
MMIO-style read) but the lowered `%0 = load <4 x double>, ptr %a` is no
longer volatile.

## Fix
Pass `match`'s captured volatile arg to `Builder.CreateAlignedLoad` via the
`isVolatile` parameter, or call `NewLoad->setVolatile(...)` after.
The `match` pattern needs to either reject non-volatile constraint or
capture it for forwarding.

## Why this matters
A user-marked `volatile` matrix load (e.g., reading from a hardware
co-processor's memory-mapped matrix register) gets converted to a plain
load that any later DCE / hoisting pass may elide.
