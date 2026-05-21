# EarlyCSE merges two atomic loads with different syncscopes

## Location
`llvm/lib/Transforms/Scalar/EarlyCSE.cpp` — `EarlyCSE::getMatchingValue` (around lines 1258-1265) plus the `LoadValue` cache (around lines 661-672).

## Root cause
When considering CSE of a load against an `AvailableLoads` entry, the only atomicity check is:

```cpp
if (MemInst.isVolatile() || !MemInst.isUnordered())
    return nullptr;
// We can't replace an atomic load with one which isn't also atomic.
if (MemInst.isLoad() && !InVal.IsAtomic && MemInst.isAtomic())
    return nullptr;
```

The `LoadValue` struct stored in `AvailableLoads` only records `IsAtomic` as a bool. There is no field for the load's **syncscope** (or its **ordering**, but for non-volatile reaches here only `unordered` is admitted by the `isUnordered()` guard). Therefore two `load atomic unordered` instructions on the same pointer with **different syncscopes** are CSE'd into the first one, silently changing the effective syncscope of the second load.

`Instruction::isIdenticalTo` (used elsewhere in EarlyCSE) *does* compare syncscope, but the load path uses a value-keyed cache that doesn't preserve that bit. The check at line 1264 only guards against losing atomicity entirely, not against losing a stricter syncscope.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"
define i32 @f(ptr %p) {
  %a = load atomic i32, ptr %p syncscope("singlethread") unordered, align 4
  %b = load atomic i32, ptr %p syncscope("system") unordered, align 4
  %r = add i32 %a, %b
  ret i32 %r
}
```

## opt diff
Before:
```
%a = load atomic i32, ptr %p syncscope("singlethread") unordered, align 4
%b = load atomic i32, ptr %p syncscope("system") unordered, align 4
%r = add i32 %a, %b
```

After `opt -passes=early-cse`:
```
%a = load atomic i32, ptr %p syncscope("singlethread") unordered, align 4
%r = add i32 %a, %a
```

The `syncscope("system")` load was dropped; the surviving load has the narrower `syncscope("singlethread")`.

## Why it's wrong
Per LangRef, the syncscope argument on an atomic operation constrains which other threads/agents see the operation as part of a synchronization sequence. Replacing a `system`-scoped load with a `singlethread`-scoped one is a strict narrowing of the synchronization domain. Even if the program is using `unordered` (so per-location values are racy), the two events are not interchangeable to downstream passes that consult syncscope.

## Suggested fix
Either (a) add `SyncScope::ID` (and `AtomicOrdering`) to `LoadValue` and require equality at lookup, or (b) bail entirely when the candidate load is atomic and its syncscope differs from the cached load.

## Status: REPRODUCIBLE (IR-level miscompile, opt-only)
