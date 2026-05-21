# `copyRangeMetadata`: silently DROPS `!range` when retyping load to non-pointer non-equal type (e.g., int → float)

**Pass surface:** `instcombine` (`combineLoadToNewType` via `copyMetadataForLoad`), also `sroa`.
**Source:** `llvm/lib/Transforms/Utils/Local.cpp` lines 3361-3383:
```cpp
void llvm::copyRangeMetadata(const DataLayout &DL, const LoadInst &OldLI,
                             MDNode *N, LoadInst &NewLI) {
  auto *NewTy = NewLI.getType();
  // Simply copy the metadata if the type did not change.
  if (NewTy == OldLI.getType()) {
    NewLI.setMetadata(LLVMContext::MD_range, N);
    return;
  }

  // Give up unless it is converted to a pointer where there is a single very
  // valuable mapping we can do reliably.
  // FIXME: It would be nice to propagate this in more ways, but the type
  // conversions make it hard.
  if (!NewTy->isPointerTy())
    return;             // <-- range silently lost
  ...
```
Caller: `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp` line 609 (`copyMetadataForLoad(*NewLoad, LI);`) inside `combineLoadToNewType`.
**Triple:** `x86_64-unknown-linux-gnu`
**Tool:** `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -S -passes=instcombine` (also `-O2`).

## Root cause

`copyRangeMetadata` only knows two cases:
1. Type unchanged → copy.
2. Old non-pointer → New pointer → derive `!nonnull` from range.

For (3) int → float, int → vector, or pointer → integer, the metadata is silently DROPPED. The acknowledged FIXME ("It would be nice to propagate this in more ways") admits the gap.

The int→float case is particularly recoverable: the range expresses a bit-pattern constraint. The float load result is `bitcast(int_value)`. We could express the floating result's possible value set as `!nofpclass` (e.g., if the integer range excludes IEEE-754 NaN bit patterns) and/or as a narrower `!range`-style constraint on the bit pattern via `!noundef` carry-through. The current code does none of this.

## Reproducer

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define float @f(ptr %p) {
  %ld = load i32, ptr %p, align 4, !range !0
  %bc = bitcast i32 %ld to float
  ret float %bc
}

!0 = !{i32 1, i32 100}
```

```
$ opt -S -passes=instcombine repro.ll
```

After:
```
define float @f(ptr %p) {
  %ld1 = load float, ptr %p, align 4
  ret float %ld1
}
```

The `!range !{1, 100}` on the original int load is GONE. The range described loaded bits ∈ [1, 100), which in IEEE-754 binary32 corresponds to a specific subset of subnormals (and excludes ±0, normals, ±inf, NaN). Front-ends like Rust or sanitizer-instrumented C++ that emit these range hints over bit-casted loads lose all of them.

## Why downstream-pass bug, not just precision loss

Two pipelines exploit this:

1. **`!nofpclass` could be derived**: the int range [1, 100) excludes the NaN bit patterns (which all have exponent = 0xFF and non-zero mantissa). A correct conversion would attach `!nofpclass nan` to the float load. Downstream `fcmp`/`fmul`/`fdiv` then folds known-NaN-impossible. The current behavior loses this fold.

2. **No `!noundef` carry**: an integer load with `!range` is generally also `!noundef` (the value is bounded so it's defined). After retype, both range AND noundef are dropped. Downstream passes that previously could assume non-poison input lose that guarantee.

## Pipeline reproducer

The IR survives `-O2`:
```
$ opt -S -O2 repro.ll
; ModuleID = ...
define float @f(ptr nocapture readonly %p) ... {
  %ld1 = load float, ptr %p, align 4
  ret float %ld1
}
```
No `!range`, no `!nofpclass`, no `!noundef`. All metadata that the front-end attached to the integer load is gone.

## Notes

- `combineLoadToNewType` is invoked by InstCombine's load-to-store-type matching and by `unpackLoadToAggregate`. Both paths now exhibit the metadata loss.
- SROA's call at `SROA.cpp:3174` exhibits the same loss when slicing alloca'd structs with mixed types.
- A correct fix could add an int→FP arm to `copyRangeMetadata` that emits `!nofpclass` based on which IEEE bit patterns the range excludes.
