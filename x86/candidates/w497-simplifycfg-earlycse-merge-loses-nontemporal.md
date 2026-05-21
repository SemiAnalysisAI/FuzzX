# SimplifyCFG → EarlyCSE: BB merging enables CSE that intersects away `!nontemporal`

## Pattern

`!nontemporal` is a performance/cache hint, not a correctness condition. When two loads of the same address — only one of which carries `!nontemporal` — sit in *different* basic blocks separated by a branch, EarlyCSE alone refuses to merge them (the memory-generation counter is bumped across the branch, and the dominating load is not "current" in the successor block). SimplifyCFG alone happily folds away an empty fall-through block but doesn't touch the loads.

Run *both* in pipeline order, however, and the loads land in the same basic block. EarlyCSE then merges them and calls `combineMetadataForCSE(KeepInst, DyingInst, /*DoesKMove=*/false)`. The `MD_nontemporal` arm of `combineMetadata` (Local.cpp:3030-3034) writes `K->setMetadata(MD_nontemporal, JMD)` unconditionally when `!AAOnly`. Because `JMD` is `nullptr` (the dying load lacked the hint), the surviving load loses its hint. The user's explicit "stream this read, don't pollute the cache" annotation is gone.

This is a textbook cross-pass interaction:
- Neither `simplifycfg` nor `early-cse` alone changes the metadata state.
- The first pass reshapes the IR (collapses an empty BB) such that the second pass becomes applicable, and the second pass's metadata-intersection rule then strips the hint.

## .ll

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%struct.S = type { i32, i32 }

define i32 @cross5(ptr %p, i1 %c) {
entry:
  %g = getelementptr %struct.S, ptr %p, i32 0, i32 0
  %a = load i32, ptr %g, align 4, !nontemporal !0
  br i1 %c, label %then, label %merge
then:
  br label %merge
merge:
  %g2 = getelementptr %struct.S, ptr %p, i32 0, i32 0
  %b = load i32, ptr %g2, align 4
  %r = add i32 %a, %b
  ret i32 %r
}

!0 = !{i32 1}
```

## opt -O2 -S diff

```
                        Stock                       After O2
                        -----                       --------
loads                   2 (one nontemporal)         1 (no nontemporal)
branch                  intact                      eliminated
metadata !0             attached to %a              dangling, unused
```

Per-pass:

| Pass(es)                          | Loads   | Nontemporal preserved? |
| --------------------------------- | ------- | ---------------------- |
| `early-cse` alone                 | 2       | yes                    |
| `simplifycfg` alone               | 2       | yes                    |
| `simplifycfg,early-cse`           | 1       | **no**                 |
| `instcombine,simplifycfg,early-cse` | 1     | **no**                 |
| `-O2`                             | 1       | **no**                 |

## Which pass triggers / which is at fault

- **Trigger**: `simplifycfg` collapses the empty `then` BB via `TryToSimplifyUncondBranchFromEmptyBlock` (Local.cpp:1155), placing both loads in one BB. **Then** `early-cse` notices that `%b` is dominated by `%a` with no intervening write and replaces it. The replacement path at `EarlyCSE.cpp:1615-1617` calls `combineMetadataForCSE(I, &Inst, false)`.
- **Actual fault**: `combineMetadata` in `Utils/Local.cpp:3030-3034`:
  ```cpp
  case LLVMContext::MD_nontemporal:
    // Preserve !nontemporal if it is present on both instructions.
    if (!AAOnly)
      K->setMetadata(Kind, JMD);
    break;
  ```
  The doc comment says "preserve if both," but the code unconditionally clears `K` when `JMD == nullptr`. `MD_nontemporal` is a *hint* — there is no correctness reason to require both loads to carry it. The fix is either to drop the intersection semantics entirely (always keep the union of hints) or to guard the strip behind a `KMD && JMD` check.
- **Why "cross-pass"**: `early-cse` standalone declines the merge because the loads are in different BBs with intervening control flow. `simplifycfg` standalone is metadata-neutral. Only the combination is destructive — `simplifycfg` is the enabler, `early-cse` is the executor, and the underlying buggy metadata-merge is in `Utils/Local.cpp`.

## Verified
- `opt -passes=early-cse -S` — two loads, nontemporal kept on `%a`.
- `opt -passes=simplifycfg -S` — two loads still distinct, both metadata intact.
- `opt -passes='simplifycfg,early-cse' -S` — one load, **no nontemporal**.
- `opt -O2 -S` — same as above (one load, no nontemporal).

## Source citations
- `llvm/lib/Transforms/Scalar/EarlyCSE.cpp:1607-1626` — load CSE replacement, invokes `combineMetadataForCSE`.
- `llvm/lib/Transforms/Scalar/EarlyCSE.cpp:1376-1381` — single-predecessor generation tracking that makes EarlyCSE *alone* refuse to merge the two loads pre-simplifycfg.
- `llvm/lib/Transforms/Utils/Local.cpp:1155` — `TryToSimplifyUncondBranchFromEmptyBlock` (the SimplifyCFG enabler).
- `llvm/lib/Transforms/Utils/Local.cpp:2934-2946` — `combineMetadata` iteration setup.
- `llvm/lib/Transforms/Utils/Local.cpp:3030-3034` — buggy `MD_nontemporal` arm.
- Compare with the `MD_invariant_load` arm at lines 2984-2988 which correctly conditions on `DoesKMove` and only sets on union (J having it).

## Fix sketch

In `Utils/Local.cpp`:
```cpp
case LLVMContext::MD_nontemporal:
  // !nontemporal is a hint, not a semantic requirement. Take the
  // union: preserve it if either instruction carried it. (Or, if the
  // intersection semantics are intended, at least respect them and
  // only clear when both are missing.)
  if (!AAOnly) {
    if (KMD || JMD)
      K->setMetadata(Kind, KMD ? KMD : JMD);
  }
  break;
```

The union semantics are safer because `!nontemporal` is purely a hint, never UB-inducing. Even if a more conservative intersection is desired, the current code is internally inconsistent with its own docstring.
