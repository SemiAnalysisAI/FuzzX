# EarlyCSE forwards a *volatile* load's value to a later regular load (LoadValue has no IsVolatile bit)

## Location
`llvm/lib/Transforms/Scalar/EarlyCSE.cpp` — `LoadValue` struct (lines 661-673), insertion path at lines 1629-1633, and `getMatchingValue` gate at lines 1261-1265.

## Root cause
`LoadValue` carries `IsAtomic` but NOT `IsVolatile`:

```cpp
// EarlyCSE.cpp:661-672
struct LoadValue {
  Instruction *DefInst = nullptr;
  unsigned Generation = 0;
  int MatchingId = -1;
  bool IsAtomic = false;
  bool IsLoad = false;
  ...
};
```

Every load is inserted into `AvailableLoads`, regardless of volatility (lines 1629-1633). The `getMatchingValue` gate only checks the *current* `MemInst`:

```cpp
// EarlyCSE.cpp:1261
if (MemInst.isVolatile() || !MemInst.isUnordered())
    return nullptr;
// We can't replace an atomic load with one which isn't also atomic.
if (MemInst.isLoad() && !InVal.IsAtomic && MemInst.isAtomic())
    return nullptr;
```

So when `InVal.DefInst` is a volatile load and the current `MemInst` is a regular (non-volatile, non-atomic) load at the same pointer, EarlyCSE returns the volatile load and the second load is eliminated. The same field that lets us guard against losing atomicity (`IsAtomic`) is missing for volatility.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"
define {i32, i32} @f(ptr %p) {
  %v1 = load volatile i32, ptr %p, align 4
  %v2 = load i32, ptr %p, align 4
  %r1 = insertvalue {i32, i32} undef, i32 %v1, 0
  %r2 = insertvalue {i32, i32} %r1, i32 %v2, 1
  ret {i32, i32} %r2
}
```

## opt diff
Before `opt -passes='early-cse<memssa>' -S`:
```
%v1 = load volatile i32, ptr %p, align 4
%v2 = load i32, ptr %p, align 4
```

After:
```
%v1 = load volatile i32, ptr %p, align 4
; %v2 is gone; insertvalue uses now read %v1 twice
```

## Why it is potentially wrong
The comment at lines 657-660 states: "atomic and/or volatile loads and stores can be present the table; it is the responsibility of the consumer to inspect the atomicity/volatility if needed." The consumer (`getMatchingValue`) inspects the *new* load's volatility but never the *cached* load's volatility. This is the symmetric form of the syncscope problem (existing w87): EarlyCSE eagerly caches without remembering that the cached entry came from an observable access, and then forwards the value as if the cache entry were neutral.

While forwarding *from* a volatile load to a regular load is generally allowed per LangRef (the volatile load is preserved and the value the regular load would have read is the same), this contradicts the consumer-inspects pattern stated in the comment, and it makes `LoadValue` strictly less informative than necessary (compare with `IsAtomic`). Any future change that introduces an additional volatility-dependent path (e.g., DSE-of-volatile-store-followed-by-load or constants-from-volatile-init) will be silently wrong because the bit is gone by the time we reach `getMatchingValue`.

## Suggested fix
Either (a) skip insertion of volatile loads into `AvailableLoads`, OR (b) add `IsVolatile` to `LoadValue` and bail symmetrically with `IsAtomic` when forwarding to a non-volatile load.

## Status
REPRODUCIBLE at IR level. The second non-volatile load is silently removed.
