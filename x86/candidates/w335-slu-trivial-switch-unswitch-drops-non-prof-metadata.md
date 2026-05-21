# w335: SimpleLoopUnswitch trivial switch unswitch silently drops !unpredictable / !make.implicit / !annotation on the hoisted switch

Pass: `simple-loop-unswitch` (trivial path, runs in default x86 -O2 as `simple-loop-unswitch<no-nontrivial;trivial>`)
File: `llvm/lib/Transforms/Scalar/SimpleLoopUnswitch.cpp`
Function: `unswitchTrivialSwitch`

## Root cause

`unswitchTrivialSwitch` constructs a fresh switch in the preheader and only copies the debug location and per-case branch weights. Any other metadata that was on the in-loop switch (`!unpredictable`, `!make.implicit`, `!annotation`, etc.) is silently dropped:

```cpp
// SimpleLoopUnswitch.cpp:925-930
// Now add the unswitched switch. This new switch instruction inherits the
// debug location of the old switch, because it semantically replace the old
// one.
auto *NewSI = SwitchInst::Create(LoopCond, NewPH, ExitCases.size(), OldPH);
NewSI->setDebugLoc(SIW->getDebugLoc());
SwitchInstProfUpdateWrapper NewSIW(*NewSI);
```

Branch weights are propagated downstream via `NewSIW.addCase(..., weight)` (line 991) and the default-weight code on lines 996-1015 — so `!prof` survives. But there is **no** generic metadata copy. `!unpredictable` and `!annotation` are unconditionally lost. `!make.implicit` is lost as well; even if preserving it across the hoist would be unsafe (this is exactly the concern the non-trivial path handles at SimpleLoopUnswitch.cpp:2372-2384, where `MD_make_implicit` is intentionally inspected/dropped based on `ICFLoopSafetyInfo`), the trivial path here does no such handling — it just discards everything.

Contrast with the non-trivial unswitch on the same file, which moves (`TI.moveBefore`) the original terminator after cloning it for MSSA bookkeeping (lines 2393-2398). That path preserves every metadata kind because the original instruction object travels with the move, and `!make.implicit` is then deliberately fixed up at 2372-2384.

## Reproducer

```llvm
; opt -passes='simple-loop-unswitch<no-nontrivial;trivial>' -S
define void @f(ptr %p, i32 %n, i32 %c) {
entry:
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %inc, %backedge ]
  switch i32 %c, label %backedge [
    i32 1, label %exit1
    i32 2, label %exit2
  ], !unpredictable !2, !annotation !3

exit1:
  ret void
exit2:
  ret void
backedge:
  %inc = add i32 %i, 1
  %cmp = icmp slt i32 %inc, %n
  br i1 %cmp, label %loop, label %exit

exit:
  ret void
}
!2 = !{}
!3 = !{!"foo"}
```

## Diff (before vs after)

Before (input):
- `switch i32 %c, label %backedge [...], !unpredictable !2, !annotation !3`

After (hoisted to entry):
- `switch i32 %c, label %entry.split [...]`   ← `!unpredictable` and `!annotation` gone

The same hoist happens with `!prof` metadata, which IS preserved (via `SwitchInstProfUpdateWrapper`). The omission is non-prof metadata.

## Impact / why it matters in -O2

- `!unpredictable` is used by codegen (`X86ISelLowering::isUnpredictable`, lowering of `BR_CC`/`SELECT_CC`) to avoid generating CMOV/branch hints that assume predictable behavior. Losing it across the hoist can flip codegen choices on x86 from the unpredictable form to the predictable form on the preheader switch.
- `!annotation` carries user/IR-level annotation strings (e.g. `auto-init`) that downstream passes and remarks key off. Silently dropping it breaks tooling/diagnostics.
- `!make.implicit` is more delicate: even when it would be correct to preserve, it is dropped; when it would be incorrect to preserve (because the faulting memory op no longer guards the branch), the trivial path has no inspection at all — symmetric to the bug that the non-trivial path explicitly guards against.

## Suggested fix

After `NewSI->setDebugLoc(SIW->getDebugLoc())` at line 929, copy non-prof metadata from `&*SIW` to `NewSI` (e.g. `copyMetadata` excluding `LLVMContext::MD_prof`), and apply the same `MD_make_implicit` safety check as the non-trivial code uses (`ICFLoopSafetyInfo::isGuaranteedToExecute`) — or unconditionally drop `MD_make_implicit` like `DropNonTrivialImplicitNullChecks` does for non-trivial unswitch.
