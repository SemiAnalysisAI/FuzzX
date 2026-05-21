# InstCombine internal load CSE drops `!nontemporal` (cross-pass amplifier)

## Pattern

`InstCombinerImpl::visitLoadInst` performs internal load CSE / store-to-load forwarding via `FindAvailableLoadedValue`. When the dying load and the surviving load have different types, InstCombine inserts a bitcast and calls `combineMetadataForCSE(AvailableVal, &LI, false)` (DoesKMove=false). That call dispatches into `combineMetadata` in `Utils/Local.cpp`, whose `MD_nontemporal` arm unconditionally writes `K->setMetadata(MD_nontemporal, JMD)` whenever `!AAOnly` is set — even when `JMD` is `nullptr`.

Net effect: `K`'s `!nontemporal` is **silently dropped** if the redundant load `J` lacked it. The hint that the user wrote ("stream this, don't pollute the cache") is erased. Worse, when `J` carries `!nontemporal` and `K` does not, the iteration is over `K`'s metadata (not `J`'s), so `J`'s hint is never transferred either. Both directions lose information.

This is a **first-pass-output-shapes-second-pass behavior** style bug because the standalone `early-cse` pass refuses to CSE the two loads (they have different types, so `getOrCreateResult` rejects). It is *only* InstCombine's internal forwarder, which is willing to insert a bitcast and bridge the type difference, that performs the merge and loses the nontemporal. Then, downstream SimplifyCFG / EarlyCSE see a single load (no nontemporal) and propagate it unchanged. In `-O2` the user's IR is forever stripped.

## .ll

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define { float, i32 } @cross_xpass(ptr %p) {
entry:
  %a = load i32,   ptr %p, align 4, !nontemporal !0
  %b = load float, ptr %p, align 4
  %v0 = insertvalue { float, i32 } poison, float %b, 0
  %v1 = insertvalue { float, i32 } %v0,    i32   %a, 1
  ret { float, i32 } %v1
}

!0 = !{i32 1}
```

## opt -O2 -S diff

Stock IR carries `!nontemporal` on `%a`. After `-O2`:

```llvm
define { float, i32 } @cross_xpass(ptr readonly captures(none) %p) {
entry:
  %a = load i32, ptr %p, align 4         ; <-- !nontemporal GONE
  %b.cast = bitcast i32 %a to float
  %v0 = insertvalue { float, i32 } poison, float %b.cast, 0
  %v1 = insertvalue { float, i32 } %v0, i32 %a, 1
  ret { float, i32 } %v1
}
```

A flipped variant (nontemporal moved to `%b`) is even more damaging — `!nontemporal` is the *only* metadata on the doomed load and it disappears entirely (neither survivor nor the bitcast carries it).

## Which pass triggers / which is at fault

- **Trigger**: `InstCombinerImpl::visitLoadInst` at `InstCombineLoadStoreAlloca.cpp:1107-1113`. `FindAvailableLoadedValue` returns `%a`, `IsLoadCSE=true`, then line 1109 calls `combineMetadataForCSE(cast<LoadInst>(AvailableVal), &LI, false)`.
- **Actual fault**: `combineMetadata` in `Utils/Local.cpp:3030-3034`:
  ```cpp
  case LLVMContext::MD_nontemporal:
    // Preserve !nontemporal if it is present on both instructions.
    if (!AAOnly)
      K->setMetadata(Kind, JMD);
    break;
  ```
  Comment claims "preserve if present on both," but the code unconditionally clears `K` when `JMD == nullptr`. There is no `K->hasMetadata(MD_nontemporal)` guard. The intersection-style behavior in the doc string is silently incorrect for the half where `K` has it and `J` doesn't (the `J`-has / `K`-doesn't half is also broken: the loop iterates K's metadata so the case never executes and J's hint is dropped on the floor).
- **Cross-pass amplifier**: `-passes=early-cse` alone declines to merge these two loads (the type mismatch fails `getOrCreateResult`). InstCombine *enables* the merge by injecting a bitcast, so the bug surfaces only when InstCombine runs in the pipeline. EarlyCSE/SimplifyCFG then propagate the now-metadata-stripped load unchanged through the rest of `-O2`.

## Verified
- `opt -passes=early-cse -S` keeps both loads and the nontemporal.
- `opt -passes=instcombine -S` drops the nontemporal (one InstCombine iteration suffices).
- `opt -O2 -S` matches the InstCombine-only output for the load itself.

## Source citations
- `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:1102-1114` — internal CSE site.
- `llvm/lib/Transforms/Utils/Local.cpp:2934-2946, 3030-3034` — `combineMetadata` iteration over K's MD, and the buggy `MD_nontemporal` arm.
- Contrast with `LLVMContext::MD_invariant_load` arm at `Local.cpp:2984-2988` which guards on `DoesKMove` (correct intersection logic) — `MD_nontemporal` lacks that guard.

## Fix sketch

`MD_nontemporal` should mirror the intersection semantics documented in the comment, e.g.:

```cpp
case LLVMContext::MD_nontemporal:
  if (!AAOnly) {
    // Preserve only if K already has it AND J has it (intersection).
    if (KMD && JMD)
      K->setMetadata(Kind, JMD);
    else
      K->setMetadata(Kind, nullptr);
  }
  break;
```

Since `!nontemporal` is purely a performance hint (not a semantic requirement), dropping it when sources disagree is safe, but the *current* code drops it whenever the J-side is missing it regardless of K, which is overly aggressive and inconsistent with the documented behavior.
