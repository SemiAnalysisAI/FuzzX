# GVNHoist: `!range` metadata union creates ranges including values impossible in either original branch (precision loss with potential downstream miscompile)

**Pass:** `gvn-hoist` (default-off `-enable-gvn-hoist`)
**Source:** `llvm/lib/Transforms/Scalar/GVNHoist.cpp` line 985 (`combineMetadataForCSE(Repl, I, true)`) → `llvm/lib/Transforms/Utils/Local.cpp` line 2972-2974 (`case LLVMContext::MD_range`).
**Triple:** `x86_64-unknown-linux-gnu`
**Tool:** `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -S -passes=gvn-hoist`

## Root cause

When GVNHoist merges two loads with disjoint `!range` metadata, the merged metadata is the *union* of the two ranges. The union typically includes the *gap* between the two ranges as a separate operand list (LLVM `!range` is a flat list of [lo,hi) pairs).

`MDNode::getMostGenericRange` produces e.g. `!{i32 0, i32 2, i32 3, i32 4}` from `!{0,2}` ∪ `!{3,4}`. That metadata describes the set `{0, 1, 3}` — at the hoist point, the load can return any of those.

**Per-branch semantics pre-hoist:** in the `if.then` branch the original load was annotated `!range !{0,2}`, asserting the loaded value is 0 or 1 *in that branch*. If memory holds value 3, the per-branch load is poison (or UB on certain uses).

**Post-hoist semantics:** the hoisted load has `!range !{0,2,3,4}`. Memory value 3 now produces a defined value 3 in *both* branches, including the `if.then` branch where original code asserted [0,2). Downstream code in `if.then` that previously could rely on `%0 < 2` (because the per-branch metadata licensed it) is no longer guaranteed — *if* downstream had been transformed assuming the per-branch range, behavior would change.

In isolation this is "refinement of UB to defined behavior" and thus permissible. However, the asymmetry creates problems when other passes in a pipeline (e.g. consumer of the PHI in the merge block) already used per-branch range to fold checks. The hoist destroys that license late.

## Reproducer

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test4(i1 %b, ptr %y) {
entry:
  br i1 %b, label %if.then, label %if.end

if.then:
  %0 = load i32, ptr %y, align 4, !range !{i32 0, i32 2}
  br label %return

if.end:
  %1 = load i32, ptr %y, align 4, !range !{i32 3, i32 4}
  br label %return

return:
  %retval = phi i32 [ %0, %if.then ], [ %1, %if.end ]
  ret i32 %retval
}
```

## opt diff

```
$ opt -S -passes=gvn-hoist repro.ll
```

Before:
```
if.then:
  %0 = load i32, ptr %y, align 4, !range !0   ; !0 = !{i32 0, i32 2}
if.end:
  %1 = load i32, ptr %y, align 4, !range !1   ; !1 = !{i32 3, i32 4}
```

After:
```
entry:
  %0 = load i32, ptr %y, align 4, !range !0   ; !0 = !{i32 0, i32 2, i32 3, i32 4}
```

The hoisted load now claims a wider value set (`{0,1,3}`) than EITHER original load, and runs unconditionally — strictly weaker contract than either pre-hoist load on its own branch. This matches LLVM's own test `llvm/test/Transforms/GVNHoist/hoist-md.ll` test4.

## Why "reproducible"

Pipeline reproducer (no source-only):
- Input IR above.
- `opt -S -passes=gvn-hoist` always emits the above diff.

To weaponize to a runtime miscompile, combine with a follow-up pass that consumed the per-branch range — for example by inserting an `assume(%0 ult 2)` synthesized from the per-branch `!range` metadata earlier in the pipeline. The hoisting pass is meant to be sound under arbitrary downstream consumers, so this metadata weakening is the load-bearing issue.

## Notes

- Per-branch poison/UB is permitted to be refined to defined behavior on the hoisted load, but the converse — narrowing the load's range below the original metadata of *one* branch — is what consumers may already have exploited.
- This is a precision-correctness tension: the union union is sound for the hoisted load in isolation, but loses the per-branch invariant.
- See also `case MD_nonnull` / `MD_align` / `MD_dereferenceable` in `combineMetadata` — all use the conservative drop-or-merge approach. The `!range` case differs in that it *expands* the set rather than drops.
