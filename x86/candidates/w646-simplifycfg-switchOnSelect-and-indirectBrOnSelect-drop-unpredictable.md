# w646 - SimplifyCFG `simplifyTerminatorOnSelect` drops `!unpredictable` when folding switch/indirectbr-on-select to a branch

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp`

- `SimplifyCFGOpt::simplifyTerminatorOnSelect` lines 4829-4904 (the new
  `CondBrInst::CreateCondBr` at line 4872 only copies branch weights, not
  `!unpredictable`).
- Callers: `SimplifyCFGOpt::simplifySwitchOnSelect` line 4910 and
  `SimplifyCFGOpt::simplifyIndirectBrOnSelect` line 4947.
- Reached from `SimplifyCFGOpt::simplifySwitch` line 8208-8210 in the
  default pass-spec.

The relevant excerpt:

```cpp
} else {
  // We found both of the successors we were looking for.
  // Create a conditional branch sharing the condition of the select.
  CondBrInst *NewBI = Builder.CreateCondBr(Cond, TrueBB, FalseBB);
  setBranchWeights(*NewBI, {TrueWeight, FalseWeight},
                   /*IsExpected=*/false, /*ElideAllZero=*/true);
}
```

There is no path that propagates the `MD_unpredictable` (or any other
metadata kind) carried on the original switch/indirectbr to `NewBI`.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

declare void @use(i32)

define void @switch_on_select(i1 %c) {
entry:
  %v = select i1 %c, i32 1, i32 2
  switch i32 %v, label %unreached [
    i32 1, label %case1
    i32 2, label %case2
  ], !unpredictable !0
case1:
  call void @use(i32 1)
  ret void
case2:
  call void @use(i32 2)
  ret void
unreached:
  unreachable
}

!0 = !{}
```

## Invocation

```
opt -passes=simplifycfg -S repro.ll
```

## Observed output

```
define void @switch_on_select(i1 %c) {
entry:
  br i1 %c, label %case1, label %case2          ; <-- !unpredictable gone
common.ret:
  ret void
case1:
  call void @use(i32 1)
  br label %common.ret
case2:
  call void @use(i32 2)
  br label %common.ret
}
```

The original switch was tagged `!unpredictable`. Its successors are still
case1 / case2 — same observable control flow — but the synthesized
`br i1` carries no metadata. `!unpredictable` is a real
hint to BPI/BFI and codegen (it disables branch-probability estimation
heuristics such as the "branches that match a pattern look biased"
defaults in `BranchProbabilityInfo`). Losing it changes layout, branch
prediction hints, and the cost models used by later passes
(`SimplifyCFGOpt`'s own predictability gates included).

The mirror bug exists in `simplifyIndirectBrOnSelect`: it builds the
new branch via the same `simplifyTerminatorOnSelect` helper, so an
`indirectbr` carrying `!unpredictable` will lose it as well when the
address operand is a `select` of `blockaddress`.

## Fix

At the `CreateCondBr` site in `simplifyTerminatorOnSelect`, propagate
`OldTerm`'s `MD_unpredictable` (and arguably `MD_annotation`):

```cpp
if (MDNode *Unpredictable =
        OldTerm->getMetadata(LLVMContext::MD_unpredictable))
  NewBI->setMetadata(LLVMContext::MD_unpredictable, Unpredictable);
```

(The unconditional-branch path at line 4868/4886/4889 cannot carry
`!unpredictable` since unconditional branches have nothing to predict;
no fix needed there.)
