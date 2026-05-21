# w424 — `foldBranchToCommonDest`/`performBranchToCommonDestFolding` drops `!unpredictable` and `!misexpect` from the inner branch when merging into the predecessor

Severity: !prof / branch-hint corruption. Not a miscompile of program
behavior, but corrupts branch-predictor and CHKP-style branch-hint metadata
that downstream passes (`MachineBlockPlacement`, `BranchFolding`,
`LoopRotation`) consume.

## Where

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp:3965-3970`
(inside `performBranchToCommonDestFolding`):

```cpp
IRBuilder<> Builder(PBI);
// The builder is used to create instructions to eliminate the branch in BB.
// If BB's terminator has !annotation metadata, add it to the new
// instructions.
Builder.CollectMetadataToCopy(BB->getTerminator(),
                              {LLVMContext::MD_annotation});
```

And `SimplifyCFG.cpp:4028-4031`:

```cpp
// If BI was a loop latch, it may have had associated loop metadata.
// We need to copy it to the new latch, that is, PBI.
if (MDNode *LoopMD = BI->getMetadata(LLVMContext::MD_loop))
  PBI->setMetadata(LLVMContext::MD_loop, LoopMD);
```

## What's wrong

`foldBranchToCommonDest` merges an inner conditional branch `BI` (in `BB`)
with a predecessor conditional branch `PBI` (in `PredBlock`) by `or`/`and`-ing
their conditions, then re-using PBI's branch. The transform explicitly
propagates:

- `!annotation` (via `Builder.CollectMetadataToCopy` at line 3969)
- `!loop` (via the explicit `setMetadata` at line 4030-4031)
- `!prof` (via the computed `MDWeights` at line 4013, when extraction succeeds)

But the following terminator-attached metadata on `BI` is **silently dropped**
and never propagated to the merged branch on PBI:

- **`!unpredictable`** — explicit branch-predictor hint that the inner branch
  is unpredictable. After the fold, the combined condition is `or`/`and` of
  the two predicates; if either source was unpredictable, the combined
  branch is at least as unpredictable. Dropping it loses the hint and lets
  later code (e.g. `isProfitableToSpeculate` at `SimplifyCFG.cpp:3150` and
  `foldTwoEntryPHINode` at `SimplifyCFG.cpp:3713`) treat the merged branch
  as predictable, gating off useful speculative transformations.
- **`!misexpect`** — diagnostic metadata produced by `__builtin_expect`
  / `__builtin_expect_with_probability` accuracy tracking. Dropping it
  silences the `-Wmisexpect` warning machinery.
- **`!make.implicit`** — frontend hint for implicit null checks.

Note that the helper's intent at line 3969 is to copy metadata onto *new
instructions Builder creates*, but PBI itself is the surviving branch and its
non-prof, non-loop, non-annotation metadata on `BI` (which is erased) is
never transferred.

## Reproducer

`/tmp/w420/t46_foldBranch_unpredictable.ll`:

```ll
declare void @l()
declare void @e()

define void @f(i1 %a, i1 %b) {
entry:
  br i1 %a, label %inner, label %end
inner:
  br i1 %b, label %left, label %end, !unpredictable !0
left:
  call void @l()
  ret void
end:
  call void @e()
  ret void
}
!0 = !{}
```

Pipeline confirmed default: `opt -passes=simplifycfg -S`. No non-default
SimplifyCFG option needed; `foldBranchToCommonDest` is invoked from the
default `simplifyCondBranch` path at `SimplifyCFG.cpp:8605`.

After:

```ll
define void @f(i1 %a, i1 %b) {
entry:
  %a.not = xor i1 %a, true
  %b.not = xor i1 %b, true
  %brmerge = select i1 %a.not, i1 true, i1 %b.not
  br i1 %brmerge, label %end, label %left
  ; !unpredictable !0 -- DROPPED
  ...
}
```

The inner `br i1 %b, ..., !unpredictable !0` was merged into the entry branch
via `(not a) or (not b)`. The `!unpredictable` annotation from the source `br`
is not on the merged branch.

## Severity / class

PGO / branch-hint metadata corruption. Concrete downstream impact:

- `MachineBlockPlacement` uses `!unpredictable` to suppress speculative
  layout choices; dropping it leads to over-aggressive layout that costs
  cycles on inputs that are actually unpredictable.
- `isProfitableToSpeculate` (`SimplifyCFG.cpp:3145-3151`) early-returns true
  when `MD_unpredictable` is present. After this drop, later runs of
  `simplifycfg` may now decline speculation that they would have allowed
  before, or vice-versa.
- `!misexpect` powers `-Wmisexpect`; loss here results in missed warnings.

Not a miscompile of program behavior.

## Notes

- Suggested fix: explicitly transfer `MD_unpredictable`, `MD_misexpect`, and
  `MD_make_implicit` from `BI` to `PBI` before erasing `BI`. The transfer
  rule for `MD_unpredictable` is conservative (if either source was
  unpredictable, the result is unpredictable); the others can be merged
  union-style.
- The same omission exists in `simplifyCondBranchToCondBranch`
  (`SimplifyCFG.cpp:4671`+) — it reads `MD_unpredictable` to gate behavior
  but doesn't propagate it onto the result.
- Compare with the explicit `MD_loop` copy at line 4030-4031, which shows
  the intended pattern.
