# w206: CodeGenPrepare::optimizeFunnelShift drops range metadata and fast-math flags

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 7689-7722
**Function:** `CodeGenPrepare::optimizeFunnelShift`

## Summary
When CGP hoists a vector funnel shift over a select-of-splatted scalars (`fsh X, Y, (select C, T, F)` -> `select C, (fsh X, Y, T), (fsh X, Y, F)`), the two new `Builder.CreateIntrinsic` calls are produced fresh: no IR flags, no call-site attributes, and no metadata are copied from the original `Fsh`. Loss is reproducible for `!range`, `!noundef`, `!align`, and similar.

## Source

```c++
IRBuilder<> Builder(Fsh);
Value *X = Fsh->getOperand(0), *Y = Fsh->getOperand(1);
Value *NewTVal = Builder.CreateIntrinsic(Opcode, Ty, {X, Y, TVal});
Value *NewFVal = Builder.CreateIntrinsic(Opcode, Ty, {X, Y, FVal});
Value *NewSel = Builder.CreateSelect(Cond, NewTVal, NewFVal);
replaceAllUsesWith(Fsh, NewSel, FreshBBs, IsHugeFunc);
Fsh->eraseFromParent();
```

There is no propagation of `Fsh`'s call-site attributes, range metadata, fast-math flags (irrelevant for integer fshl/fshr but relevant for any future fp-funnel intrinsic), or the original parameter attributes.

## Reproducer (`test_funnelshift.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare <4 x i32> @llvm.fshl.v4i32(<4 x i32>, <4 x i32>, <4 x i32>)

define <4 x i32> @test(<4 x i32> %x, <4 x i32> %y, i1 %c, i32 %t, i32 %f) {
entry:
  %tv = insertelement <4 x i32> poison, i32 %t, i32 0
  %ts = shufflevector <4 x i32> %tv, <4 x i32> poison, <4 x i32> zeroinitializer
  %fv = insertelement <4 x i32> poison, i32 %f, i32 0
  %fs = shufflevector <4 x i32> %fv, <4 x i32> poison, <4 x i32> zeroinitializer
  %sel = select i1 %c, <4 x i32> %ts, <4 x i32> %fs
  %r = call <4 x i32> @llvm.fshl.v4i32(<4 x i32> %x, <4 x i32> %y, <4 x i32> %sel), !range !0
  ret <4 x i32> %r
}

!0 = !{i32 0, i32 32}
```

## Reproduce
```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -stop-after=codegenprepare \
    test_funnelshift.ll -o -
```

Observed IR after CGP:
```llvm
%0 = call <4 x i32> @llvm.fshl.v4i32(<4 x i32> %x, <4 x i32> %y, <4 x i32> %ts)
%1 = call <4 x i32> @llvm.fshl.v4i32(<4 x i32> %x, <4 x i32> %y, <4 x i32> %fs)
%2 = select i1 %c, <4 x i32> %0, <4 x i32> %1
```

Neither call carries `!range !0` from the original.

## Suggested fix
After creating each `NewTVal`/`NewFVal`, copy IR flags and attributes from `Fsh`:
```c++
if (auto *I = dyn_cast<Instruction>(NewTVal)) {
  I->copyIRFlags(Fsh);
  I->copyMetadata(*Fsh);
}
if (auto *I = dyn_cast<Instruction>(NewFVal)) {
  I->copyIRFlags(Fsh);
  I->copyMetadata(*Fsh);
}
```

## Impact
Lost optimization information for downstream consumers (DAGCombine, simplifications) that rely on `!range` to narrow operations.
