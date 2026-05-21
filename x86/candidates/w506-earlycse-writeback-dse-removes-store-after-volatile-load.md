# EarlyCSE writeback-DSE removes a non-volatile store that follows a *volatile* load

## Location
`llvm/lib/Transforms/Scalar/EarlyCSE.cpp` — `processNode` writeback-DSE block at lines 1728-1747, which calls `getMatchingValue` at line 1731-1732. The check at line 1261 (`if (MemInst.isVolatile() || !MemInst.isUnordered())`) only inspects the current store; nothing inspects whether the cached `InVal.DefInst` was a volatile load.

## Root cause
`LoadValue` does not store `IsVolatile` (lines 661-673), and the writeback-DSE block treats the cached entry as a neutral source-of-truth. When the cached `InVal.DefInst` is `load volatile`, and the current `MemInst` is a non-volatile store of the loaded value back to the same pointer, the call chain proceeds:

* `MemInst.isVolatile()` is false (the store is non-volatile); `MemInst.isUnordered()` is true.
* Line 1264 only checks load atomicity (MemInst.isLoad() is false here — MemInst is the store).
* `MemInstMatching = !MemInst.isLoad() = true`. `Matching = MemInst` (the store), `Other = InVal.DefInst` (the volatile load).
* `getOrCreateResult(Matching=store, Other->getType())` returns `store->getValueOperand()` if types match.
* If the store's value operand is exactly the load itself, `InVal.DefInst == Result`, and we proceed.
* Generation check passes (no intervening writes).
* `getMatchingValue` returns `InVal.DefInst` (the volatile load).
* Line 1730: `InVal.DefInst == getMatchingValue(...)`, so the store is DSE'd.

The volatile load is preserved (it is the kept cached value), but the program goes from "read once volatile, then store back" to "read once volatile, no store". The non-volatile store is dropped.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"
define void @f(ptr %p) {
  %v = load volatile i32, ptr %p, align 4
  store i32 %v, ptr %p, align 4
  ret void
}
```

## opt diff
Before:
```
%v = load volatile i32, ptr %p, align 4
store i32 %v, ptr %p, align 4
ret void
```

After `opt -passes='early-cse<memssa>' -S`:
```
%v = load volatile i32, ptr %p, align 4
ret void
```

## Why it is wrong
For MMIO patterns the C programmer typically uses `volatile` to denote the load (e.g., reading a status register) and may follow it with a deliberate write-back to clear-on-write or to acknowledge the read. The IR pattern above describes exactly that, with the store left non-volatile because the programmer intends a "best-effort" write that the compiler can move around but not delete.

Even in non-MMIO contexts, the non-volatile store is a real memory write that another thread reading the same location via plain (non-atomic, non-volatile) accesses could observe — strictly speaking those accesses race with the store and are UB, so this argument is weak. The MMIO case is stronger: there, a store removal is a behavioral change visible to the hardware.

The fundamental defect is symmetric to the `IsVolatile`-missing analysis in w505: `getMatchingValue`'s writeback path treats `InVal.DefInst` as a neutral abstract source of the value, but a `load volatile` is not a neutral source — it is itself an observable operation, and using it to silently delete *another* observable operation (the store) is a step further than just CSEing two reads.

## Suggested fix
The same as w505: add `IsVolatile` to `LoadValue` and refuse writeback-DSE in `getMatchingValue` (or in the caller at line 1730-1746) when the cached load is volatile and the candidate store is not.

## Status
REPRODUCIBLE. The non-volatile store is removed by EarlyCSE.
