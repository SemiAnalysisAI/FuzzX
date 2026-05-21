# w337: LoopRotation::updateBranchWeights silently demotes `branch_weights` with `expected` origin

Pass: `loop-rotate` (runs in default x86 -O2 as `loop-rotate<header-duplication;no-prepare-for-lto;no-check-exit-count>`)
File: `llvm/lib/Transforms/Utils/LoopRotationUtils.cpp`
Function: `updateBranchWeights`

## Root cause

After loop rotation, the cloned/preheader branch and the latch branch get fresh weights computed from the original header weights. Both writes hard-code `IsExpected=false`, regardless of whether the original metadata was tagged `!"expected"` (from `__builtin_expect` / `llvm.expect`).

```cpp
// LoopRotationUtils.cpp:325-336
const uint32_t LoopBIWeights[] = {
    SuccsSwapped ? LoopBackWeight : ExitWeight1,
    SuccsSwapped ? ExitWeight1 : LoopBackWeight,
};
setBranchWeights(LoopBI, LoopBIWeights, /*IsExpected=*/false);
if (HasConditionalPreHeader) {
  const uint32_t PreHeaderBIWeights[] = {
      SuccsSwapped ? EnterWeight : ExitWeight0,
      SuccsSwapped ? ExitWeight0 : EnterWeight,
  };
  setBranchWeights(PreHeaderBI, PreHeaderBIWeights, /*IsExpected=*/false);
}
```

`setBranchWeights` overwrites any existing `prof` node (see `llvm/IR/ProfDataUtils.h:148-154`), so the `expected` tag — i.e. the second metadata string `!"expected"` that distinguishes user-asserted hints from sampling estimates — is dropped on every rotated loop with `__builtin_expect`-derived weights.

`hasBranchWeightOrigin` exists for exactly this query and is not used here.

## Reproducer

```llvm
; opt -passes='loop-mssa(loop-rotate)' -S
define void @f(ptr %p, i32 %n) {
entry:
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %inc, %latch ]
  %cmp = icmp slt i32 %i, %n
  br i1 %cmp, label %latch, label %exit, !prof !0

latch:
  store i32 %i, ptr %p, align 4
  %inc = add i32 %i, 1
  br label %loop

exit:
  ret void
}
!0 = !{!"branch_weights", !"expected", i32 100, i32 1}
```

## Diff

Before:
- `!0 = !{!"branch_weights", !"expected", i32 100, i32 1}` (origin == expected)

After loop-rotate:
- header guard:  `!0 = !{!"branch_weights", i32 127, i32 1}`        (no `"expected"`)
- latch backedge: `!1 = !{!"branch_weights", i32 12673, i32 127}`   (no `"expected"`)

Both rotated metadata nodes have lost the `!"expected"` second operand. Any downstream consumer that distinguishes expected from sampled weights (e.g. `extractBranchWeights` overloads that report `IsExpected`, `BranchProbabilityInfo`, profile use heuristics, llvm-readobj/diagnostics, sample PGO drift detection) now sees a sampled hint where the user wrote `__builtin_expect`.

## Impact / why it matters in -O2

- `loop-rotate` runs unconditionally at default -O2.
- `__builtin_expect` is widely used in performance-critical code (kernels, hot loop guards). Down-grading those weights to "sampled" loses the strong-hint semantics: passes that bias more aggressively for expected hints (and conversely refuse to override them with sampling data) will now treat the post-rotate hint as ordinary frequency data.
- For loops where `__builtin_expect(cond, 1)` was used to mark a hot backedge, the *very rotation that exposes the latch backedge to the rest of the pipeline* is what erases the user's hint.

## Suggested fix

In `updateBranchWeights`, capture the origin once from `WeightMD`:

```cpp
bool IsExpected = hasBranchWeightOrigin(WeightMD);
...
setBranchWeights(LoopBI, LoopBIWeights, /*IsExpected=*/IsExpected);
...
setBranchWeights(PreHeaderBI, PreHeaderBIWeights, /*IsExpected=*/IsExpected);
```

`hasBranchWeightOrigin(const MDNode*)` is already declared in `llvm/IR/ProfDataUtils.h:85`.
