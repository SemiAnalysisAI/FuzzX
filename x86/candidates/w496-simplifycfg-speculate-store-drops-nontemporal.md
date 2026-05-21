# SimplifyCFG `speculativelyExecuteBB` drops `!nontemporal` on the hoisted store (EarlyCSE then DSEs the prior store, so the nontemporal hint is permanently lost)

## Pattern

`SimplifyCFGOpt::speculativelyExecuteBB` if-converts a conditional store by hoisting it past the branch and selecting between the new and prior value. After hoisting, line 3386 of `SimplifyCFG.cpp` runs `I.dropUBImplyingAttrsAndMetadata()` on every speculated instruction. That helper (`Instruction.cpp:586`) only keeps `MD_annotation, MD_range, MD_nonnull, MD_align, MD_fpmath, MD_prof`. **`MD_nontemporal` is not in the keep list**, so the hint is silently erased from the speculated store.

`!nontemporal` is not UB-implying — it is a performance/cache hint. Stripping it is conservative for correctness but wrong as a class: the same metadata is *kept* by `combineMetadata` for CSE-style merges and by the normal hoist-common-code path. Speculation is the only path that demolishes it.

The cross-pass amplifier is that EarlyCSE/InstCombine downstream will DSE the dummy "prior" store that was responsible for making `isSafeToSpeculateStore` succeed in the first place. The original IR encoded "non-streaming initial store, conditional streaming overwrite"; the `-O2` output is a single non-streaming store. The user's only nontemporal in the function vanished without warning.

## .ll

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @cond_store_spec(ptr %p, i1 %c) {
entry:
  store i32 0, ptr %p, align 4                     ; prior store enables speculation
  br i1 %c, label %then, label %merge
then:
  store i32 42, ptr %p, align 4, !nontemporal !0   ; streaming store
  br label %merge
merge:
  ret void
}

!0 = !{i32 1}
```

## opt -O2 -S diff

After `-passes=simplifycfg` alone:
```llvm
entry:
  store i32 0, ptr %p, align 4
  %spec.store.select = select i1 %c, i32 42, i32 0
  store i32 %spec.store.select, ptr %p, align 4    ; <-- !nontemporal GONE
  ret void
```

After full `-O2`:
```llvm
entry:
  %spec.store.select = select i1 %c, i32 42, i32 0
  store i32 %spec.store.select, ptr %p, align 4    ; <-- prior store DSE'd by EarlyCSE
  ret void
```

The original function had `!nontemporal` on exactly one store. The optimized function has zero `!nontemporal` annotations and no `!0` metadata node at all.

## Which pass triggers / which is at fault

- **Trigger**: `SimplifyCFGOpt::speculativelyExecuteBB` at `SimplifyCFG.cpp:3204`. The strip happens in the `for (auto &I : make_early_inc_range(*ThenBB))` loop at `SimplifyCFG.cpp:3382-3393`, specifically `I.dropUBImplyingAttrsAndMetadata()` on line 3386.
- **Actual fault**: `Instruction::dropUBImplyingAttrsAndMetadata` at `IR/Instruction.cpp:586-602` uses a `KnownIDs` keep-list (`MD_annotation, MD_range, MD_nonnull, MD_align, MD_fpmath, MD_prof`) that omits `MD_nontemporal`. `MD_nontemporal` is a performance hint, not UB-related — stripping it on speculation is unnecessary. Either the keep-list should include `MD_nontemporal`, or SimplifyCFG should keep it itself the way it threads `MD_range` through `MaskedLoadStore->addRangeRetAttr` at `SimplifyCFG.cpp:1813-1814`.
- **Cross-pass amplifier**: 
  - `early-cse` alone is unable to merge the two stores (they live in different basic blocks → different memory generations / different `Inst` chain), so it leaves them intact.
  - `simplifycfg` alone speculates and drops the nontemporal.
  - With the full pipeline (`-O2`), `early-cse` afterward DSEs the now-dead "prior" store, leaving only the metadata-stripped one. The user's hint disappears irrecoverably.

## Verified
- `opt -passes=early-cse -S` leaves the IR exactly as written.
- `opt -passes=simplifycfg -S` produces an unconditional, **non-nontemporal** store with a select.
- `opt -O2 -S` matches the simplifycfg output with the prior store further DSEd.

## Source citations
- `llvm/lib/Transforms/Utils/SimplifyCFG.cpp:3204` — `speculativelyExecuteBB` start.
- `llvm/lib/Transforms/Utils/SimplifyCFG.cpp:3382-3393` — the per-instruction metadata strip loop.
- `llvm/lib/Transforms/Utils/SimplifyCFG.cpp:3386` — `I.dropUBImplyingAttrsAndMetadata()` call.
- `llvm/lib/IR/Instruction.cpp:586-602` — `dropUBImplyingAttrsAndMetadata` keep-list (missing `MD_nontemporal`).
- Compare with `SimplifyCFG.cpp:1813-1814` where `MD_range` is explicitly threaded through during conditional-faulting hoist; `MD_nontemporal` is not.

## Fix sketch

Option A — fix at the strip site so all speculation paths benefit:
```cpp
// IR/Instruction.cpp
static const unsigned KnownIDs[] = {
    LLVMContext::MD_annotation, LLVMContext::MD_range,
    LLVMContext::MD_nonnull,    LLVMContext::MD_align,
    LLVMContext::MD_fpmath,     LLVMContext::MD_prof,
    LLVMContext::MD_nontemporal,  // <-- hint, not UB-implying
};
```

Option B — caller-side keep, mirroring how `MD_range` is threaded:
```cpp
// SimplifyCFG.cpp:3382 loop
for (auto &I : make_early_inc_range(*ThenBB)) {
  if (!SpeculatedStoreValue || &I != SpeculatedStore) {
    I.dropLocation();
  }
  I.dropUBImplyingAttrsAndMetadata(/*Keep=*/{LLVMContext::MD_nontemporal});
  ...
}
```
