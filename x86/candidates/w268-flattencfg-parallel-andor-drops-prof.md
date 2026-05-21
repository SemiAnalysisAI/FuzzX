# w268: FlattenCFG `FlattenParallelAndOr` drops `!prof` from the first condition's branch

## Summary

`FlattenCFGOpt::FlattenParallelAndOr` collapses `if (a) if (b) X` into
`if (a && b) X` (and the `or`-dual). During the rewrite it erases the first
condition block's branch (with its `!prof`) and splices in the inner block,
keeping only the inner branch's `!prof`. The combined branch should carry
recomputed weights.

## Source

- `llvm/lib/Transforms/Utils/FlattenCFG.cpp:274-302` — the main rewrite loop:
  ```cpp
  do {
    CB = PBI->getSuccessor(1 - Idx);
    FirstCondBlock->back().eraseFromParent();      // erases first branch + !prof
    FirstCondBlock->splice(FirstCondBlock->end(), CB);
    PBI = cast<CondBrInst>(FirstCondBlock->getTerminator()); // CB's branch
    ...
    NC = Builder.CreateOr(PC, CC) | Builder.CreateAnd(PC, CC);
    PBI->replaceUsesOfWith(CC, NC);
    PC = NC;
    ...
  } while (Iteration);
  ```
  Only `PBI`'s original `!prof` (i.e. the last condition's) survives. The
  first/intermediate conditions' branch-weight metadata is silently dropped.

## Reproducer

`/home/orenamd@semianalysis.com/FuzzX/x86/candidates/w268-flattencfg-parallel-andor-drops-prof.ll`

```llvm
@g = external global i32

define void @test_parand(i1 %a, i1 %b, i32 %z) {
entry:
  br i1 %a, label %bb_b, label %exit, !prof !0   ; {1, 99}

bb_b:
  br i1 %b, label %if_then, label %exit, !prof !1 ; {30, 70}

if_then:
  store i32 %z, ptr @g, align 4
  br label %exit

exit:
  ret void
}
!0 = !{!"branch_weights", i32 1, i32 99}
!1 = !{!"branch_weights", i32 30, i32 70}
```

`opt -passes=flatten-cfg -S`:
```llvm
define void @test_parand(i1 %a, i1 %b, i32 %z) {
entry:
  %0 = and i1 %a, %b
  br i1 %0, label %if_then, label %exit, !prof !0  ; only {30, 70} kept
  ...
}
!0 = !{!"branch_weights", i32 30, i32 70}
```

The `{1, 99}` weight (P(a)=1%) on the outer condition is dropped. The correct
combined `P(a && b)` here is `0.01 * 0.30 = 0.003` → weights roughly
`{3, 997}`, not `{30, 70}`.

## Caveats

Same as w267: `flatten-cfg` is not in default x86 `-O2`. Profile-fidelity bug
rather than miscompile.

## Fix sketch

Track and combine `!prof` weights across the iterations of the rewrite loop;
set the combined weight on the final branch.
