# w63b - LowerMatrixIntrinsics dot-product flatten drops volatile on matrix.column.major.load

## Location

`llvm/lib/Transforms/Scalar/LowerMatrixIntrinsics.cpp` lines ~1701-1726
in the dot-product matmul fusion path. In the `FlattenArg` lambda:

```cpp
if (match(Op, m_Intrinsic<Intrinsic::matrix_column_major_load>(
                  m_Value(Arg)))) {
  auto *NewLoad = Builder.CreateLoad(Op->getType(), Arg);   // <-- non-volatile!
  Op->replaceAllUsesWith(NewLoad);
  eraseFromParentAndRemoveFromShapeMap(cast<Instruction>(Op));
  return;
}
```

`@llvm.matrix.column.major.load.*` takes an `i1 immarg %isVolatile` as its
3rd argument. The `m_Intrinsic<...>(m_Value(Arg))` matcher only binds the
ptr operand and never inspects the volatile flag. `Builder.CreateLoad`
without a `Volatile` argument always creates a *non-volatile* load.

Concretely: when the matmul fusion picks the "flatten / dot-product"
lowering (triggered by `m_One()` stride in the `CanBeFlattened`
predicate), a `volatile` row-vector load is rewritten to a plain load.
The volatile semantics — "do not eliminate, do not coalesce, must
observably execute exactly once at this point" — are silently dropped.
Later passes (DSE, GVN, LICM) are then free to delete or reorder the
load, which is a miscompile for memory-mapped I/O.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

declare <4 x float> @llvm.matrix.column.major.load.v4f32.i64(ptr nocapture, i64, i1 immarg, i32 immarg, i32 immarg)
declare void @llvm.matrix.column.major.store.v1f32.i64(<1 x float>, ptr nocapture, i64, i1 immarg, i32 immarg, i32 immarg)
declare <1 x float> @llvm.matrix.multiply.v1f32.v4f32.v4f32(<4 x float>, <4 x float>, i32 immarg, i32 immarg, i32 immarg)

define void @matmul_fuse(ptr %a, ptr %b, ptr %c) {
  ; A is a 1x4 row vector loaded with isVolatile=true
  %A = call <4 x float> @llvm.matrix.column.major.load.v4f32.i64(ptr %a, i64 1, i1 true,  i32 1, i32 4)
  %B = call <4 x float> @llvm.matrix.column.major.load.v4f32.i64(ptr %b, i64 4, i1 false, i32 4, i32 1)
  %res = call reassoc <1 x float> @llvm.matrix.multiply.v1f32.v4f32.v4f32(<4 x float> %A, <4 x float> %B, i32 1, i32 4, i32 1)
  call void @llvm.matrix.column.major.store.v1f32.i64(<1 x float> %res, ptr %c, i64 1, i1 false, i32 1, i32 1)
  ret void
}
```

## Invocation

```
opt -passes=lower-matrix-intrinsics -S repro.ll
```

## Observed output (excerpt)

```
define void @matmul_fuse(ptr %a, ptr %b, ptr %c) {
  %col.load = load <4 x float>, ptr %b, align 4
  %1 = load <4 x float>, ptr %a, align 16          ; <-- volatile dropped!
  %2 = fmul <4 x float> %1, %col.load
  %3 = call reassoc float @llvm.vector.reduce.fadd.v4f32(float 0.000000e+00, <4 x float> %2)
  %4 = insertelement <1 x float> poison, float %3, i64 0
  %split = shufflevector <1 x float> %4, <1 x float> poison, <1 x i32> zeroinitializer
  store <1 x float> %split, ptr %c, align 4
  ret void
}
```

The load `%1 = load <4 x float>, ptr %a, align 16` should be
`load volatile <4 x float>, ptr %a`. The non-volatile `%col.load` load
of `%b` is correct; the bug only affects the operand that was originally
declared volatile in the matrix intrinsic.

## Fix

`CreateLoad` should be passed the `i1 immarg` from arg index 2 of the
intrinsic. The matcher in `m_Intrinsic<...>` is structural-only and
ignores the volatile flag, so it needs to be extracted explicitly:

```cpp
auto *MatLoad = cast<IntrinsicInst>(Op);
bool IsVolatile = cast<ConstantInt>(MatLoad->getArgOperand(2))->isOne();
auto *NewLoad = Builder.CreateLoad(Op->getType(), Arg, IsVolatile);
```

(and similarly for an alignment hint via `getParamAlign(0)`.)

## Family

Same defect class as bugs 108 (DSE partial merge), 109 (memcpyopt-memsetmemcpy
drops volatile memset), 111 (LowerAtomic drops volatile on RMW/cmpxchg),
114 (GVNSink merges volatile stores) — passes that build IR with
`IRBuilder::CreateLoad/CreateStore` and forget to propagate the
volatile bit from the original IR.
