# 247 — ConstantFolding `foldConstVectorToAPInt` silently treats poison lanes as zero in `bitcast <NxiK> to iN*K`

Component: `llvm/lib/Analysis/ConstantFolding.cpp` lines ~79-107 (`foldConstVectorToAPInt`)

```cpp
for (unsigned i = 0; i != NumSrcElts; ++i) {
  Constant *Element = ...;
  if (isa_and_nonnull<UndefValue>(Element)) {
    Result <<= BitShift;     // BUG: treats poison as 0
    continue;
  }
  ...
}
```

`PoisonValue` is a subclass of `UndefValue`, so `isa<UndefValue>(Element)` matches both. `Result <<= BitShift; continue;` silently uses 0 for the poison lane and the caller returns `ConstantInt::get(DestTy, Result)` — poison silently demoted to zero bits.

Contrast: the byte-source path at lines 196-199 correctly returns `PoisonValue::get(DestTy)` if any source element is poison.

## Reproducer

```ll
%b = bitcast <2 x i16> <i16 0, i16 poison> to i32   ; should be poison
%c = icmp eq i32 %b, 0                              ; should be poison
```

`opt -passes=instsimplify -S` → `ret i1 true`. Expected: `ret i1 poison`.

## Severity

Real Alive2-falsifiable miscompile in default `-O2` (instsimplify runs inside InstCombine, which runs throughout the pipeline). Poison silently demoted to zero, downstream `icmp` returns concrete value.

## Fix

In `foldConstVectorToAPInt`, when an element is `UndefValue` AND `isa<PoisonValue>(Element)` is true, return `PoisonValue::get(DestTy)` (or `std::nullopt`) instead of continuing with `<<=`.
