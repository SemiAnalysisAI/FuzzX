# EarlyCSE store-to-load forwarding drops the eliminated load's metadata (!range, !nonnull, !noundef, !align)

## Location
`llvm/lib/Transforms/Scalar/EarlyCSE.cpp` — load CSE block at lines 1606-1626, specifically the gate at line 1615 (`if (InVal.IsLoad)`).

```cpp
// EarlyCSE.cpp:1615
if (InVal.IsLoad)
    if (auto *I = dyn_cast<Instruction>(Op))
        combineMetadataForCSE(I, &Inst, false);
if (!Inst.use_empty())
    Inst.replaceAllUsesWith(Op);
```

## Root cause
The gate `if (InVal.IsLoad)` runs `combineMetadataForCSE` only when forwarding load→load. When forwarding **store→load** (InVal is a previous store whose value is being reused for the current load), `combineMetadataForCSE` is **not** invoked. The current load's `!range`, `!nonnull`, `!noundef`, `!align`, `!nofpclass` metadata is then simply lost: the replacement value `Op` (the stored value) might itself be an Instruction whose metadata could be intersected/unioned with the eliminated load's metadata, but the code does not attempt that.

This is more impactful than it sounds because store-to-load forwarding is one of EarlyCSE's most common transformations.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(ptr %p, ptr %q) {
  %loaded = load i32, ptr %p, !range !0       ; range [0, 100)
  store i32 %loaded, ptr %q
  %v = load i32, ptr %q, !range !1            ; range [0, 10)
  ret i32 %v
}

!0 = !{i32 0, i32 100}
!1 = !{i32 0, i32 10}
```

## opt diff
Before `opt -passes='early-cse<memssa>' -S`:
```
%loaded = load i32, ptr %p, !range !0   ; [0, 100)
store i32 %loaded, ptr %q
%v = load i32, ptr %q, !range !1        ; [0, 10)
ret i32 %v
```

After:
```
%loaded = load i32, ptr %p, !range !0   ; still [0, 100)
store i32 %loaded, ptr %q
ret i32 %loaded                           ; uses now see %loaded with [0, 100)
```

The `!range [0, 10)` that was on `%v` is gone. The return value uses `%loaded` with the wider `[0, 100)` range, even though both loads were proved to return the same value, so the intersection `[0, 100) ∩ [0, 10) = [0, 10)` is the tighter provable fact.

## Why it is wrong
Store-to-load forwarding establishes that `%loaded == %v`. Therefore *both* `!range` annotations are simultaneously true at runtime. The intersection `[0, 10)` is the correct merged range; the post-CSE IR claims only `[0, 100)`. This permanently loses information that was available in the source IR. The information loss propagates: e.g., a later peephole that needs to prove "result is in 4 bits" or "result is never negative" might rely on the tighter range.

The same issue applies to `!nonnull` on the eliminated load (lost), `!noundef` on the eliminated load (lost), `!align` on the eliminated load (lost).

This is structurally similar to w507 (call CSE) and w508 (GEP CSE), but for a different and arguably hotter EarlyCSE path: store-to-load forwarding.

## Suggested fix
Drop the `InVal.IsLoad` gate at line 1615 (or extend it): when `Op` is an Instruction, call `combineMetadataForCSE(cast<Instruction>(Op), &Inst, /*DoesKMove=*/false)` regardless of whether InVal is a load or a store. The helper itself is robust against missing metadata; calling it more broadly only intersects where both sides actually carry the kind.

## Status
REPRODUCIBLE. The `!range [0, 10)` on the eliminated load is silently dropped, and the surviving value's range is `[0, 100)` instead of the provably-correct `[0, 10)`.
