# w305: foldVectorSelect shuffle rewrite drops FMF on newly created select

## Bug

`InstCombinerImpl::foldVectorSelect` (file:
`llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp`) recognizes
`select Cond, (shuf_sel X, Y), X|Y` (and the mirrored false-arm case)
and rewrites it to `shuf_sel(..., select Cond, ..., ...)`. The new
inner `select` is built via:

```cpp
Value *NewSel = Builder.CreateSelect(Cond, X|Y, Y|X, "sel", &Sel);
```

at lines 3074, 3079, 3088, 3093.

The `MDFrom` argument (`&Sel`) only causes `!prof` / `!unpredictable`
metadata to be copied (see `IRBuilderBase::CreateSelectFMF` in
`llvm/lib/IR/IRBuilder.cpp:1112-1127`). The FMF from the original
select is NOT copied to the new select, even though for FP-typed
vectors the inner select is a `select` over `<N x float>` (or any FP
type) which can legitimately carry FMF.

The outer Sel has FMF set. The new inner select is created with
whatever FMF is currently set on `Builder` (which is empty by default
in `visitSelectInst` — there is no `FastMathFlagGuard` set up at this
level of `visitSelectInst`).

This is a missed optimization (not a miscompile) because dropping
FMF is conservative.

## Repro

```llvm
; opt -passes=instcombine -S w305.ll
define <4 x float> @vsel(<4 x i1> %c, <4 x float> %x, <4 x float> %y) {
  %sh = shufflevector <4 x float> %x, <4 x float> %y,
                      <4 x i32> <i32 0, i32 5, i32 2, i32 7>
  %sel = select nnan ninf nsz <4 x i1> %c,
                <4 x float> %sh, <4 x float> %x
  ret <4 x float> %sel
}
```

Output (no FMF on the new inner select):
```llvm
define <4 x float> @vsel(<4 x i1> %c, <4 x float> %x, <4 x float> %y) {
  %sel1 = select <4 x i1> %c, <4 x float> %y, <4 x float> %x
  %sel = shufflevector <4 x float> %x, <4 x float> %sel1,
                       <4 x i32> <i32 0, i32 5, i32 2, i32 7>
  ret <4 x float> %sel
}
```

Expected (FMF intersected with arms, propagated):
```llvm
%sel1 = select nnan ninf nsz <4 x i1> %c, <4 x float> %y, <4 x float> %x
```

## Source cite

- `llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp:3068-3096`
  (foldVectorSelect "select shuffle" rewrite — 4 sites)
- `llvm/lib/IR/IRBuilder.cpp:1107-1127`
  (CreateSelect / CreateSelectFMF — `MDFrom` does not propagate FMF)

## Severity

Missed-opt (low severity). Subsequent users of `%sel1` that key on
FMF (e.g. another min/max canonicalization, fneg propagation, scalar
copysign formation) may fail to fire.

## Fix sketch

Add a `cast<Instruction>(NewSel)->setFastMathFlags(Sel.getFastMathFlags())`
call after the `CreateSelect` when `Sel` is an `FPMathOperator` — or
switch to `CreateSelectFMF(..., FMFSource(&Sel), ...)`. Same pattern as
the explicit FMF set used by `foldSelectOpOp` at line 379.
