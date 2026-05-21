# StackProtector: epilogue check inserted after a tail-call when a speculatable instruction sits between the call and the ret

File: llvm/lib/CodeGen/StackProtector.cpp
Function: `InsertStackProtectors` (lines 603-744)

## The pattern

The "insert canary check before tail-call" hoist only walks ONE instruction
back from the return:

```cpp
// If we're instrumenting a block with a tail call, the check has to be
// inserted before the call rather than between it and the return.
Instruction *Prev = CheckLoc->getPrevNode();
if (auto *CI = dyn_cast_if_present<CallInst>(Prev))
  if (CI->isTailCall() && isInTailCallPosition(*CI, TLI.getTargetMachine()))
    CheckLoc = Prev;
```

`isInTailCallPosition` (CodeGen/Analysis.cpp:539-586) is more permissive than
"call must be the immediate predecessor of the ret". It accepts:

* sext/zext/freeze/bitcast/getelementptr of the call result that ends up in
  the return value;
* any other side-effect-free, non-memory speculatable instruction between
  the call and the terminator;
* `lifetime_end` / `assume` / `experimental_noalias_scope_decl` / `fake_use`
  intrinsics.

So for IR like:

```llvm
define i64 @bar() sspstrong {
  %buf = alloca [16 x i8]
  store volatile i8 1, ptr %buf
  %t = tail call signext i32 @foo()
  %x = sext i32 %t to i64
  ret i64 %x
}
```

`CheckLoc = ret`, `Prev = sext` (not the call), so the `Prev == CallInst &&
isTailCall` test fails and `CheckLoc` stays on the ret. The canary
load/cmp/branch (and the `__stack_chk_fail` call in the FailBB) are inserted
between the `sext` and the `ret`.

If a backend later honors the tail-call (musttail; AArch64; ARM; targets that
allow a `sext`-of-result between a tail call and the matching ret) the call
becomes a `jmp` / `b` and never returns. The canary check that was emitted
after the call is then either (a) never executed (silently disabling stack
protection for that return path), or (b) executes the load/cmp/branch *after*
control has been transferred — depending on how the lowering plays out.

## Why X86 hides it most of the time

X86's `IsEligibleForTailCallOptimization` is conservative and currently
declines the tail-call any time there's an `sext`/`zext`/`bitcast` of the
call result interposed before the ret. So on x86-64 the asm shows a regular
`callq foo@PLT` followed by the canary check followed by `cltq; retq` — i.e.
the check is after a real call return, which is safe.

But:

1. `musttail` requires the backend to honor the tail call. The LLVM verifier
   constrains musttail to require an immediate `ret` of the call's exact
   value, which is the only reason this is not already exploitable on x86.
   Any future relaxation (e.g. allowing a `bitcast` or `freeze` after a
   musttail of pointer return) would surface the bug immediately, because
   the IR-level guard would still miss it.

2. Other backends (AArch64, ARM) are more permissive: a `sext`/`zext`
   of the result that becomes the return value is folded into the tail
   jump and the call is still emitted as `b foo`. The same IR sent through
   those backends will have the canary check trail the tail jump.

3. The file is `llvm/lib/CodeGen/`, not `llvm/lib/Target/X86/`, so the
   pessimistic-on-X86 alignment is a coincidence, not the design.

## Suggested fix

Replace the single `getPrevNode()` lookup with a real walk: find the last
non-debug, non-pseudo-probe instruction in the block prior to `CheckLoc`,
ignoring the same instructions that `isInTailCallPosition` ignores; only
*then* test for `isTailCall && isInTailCallPosition`. Equivalently, walk
the block once and remember the last tail-callable call instruction whose
result feeds the return.

## Confidence

Medium. The bug is structurally present in the IR-level pass, but only
exploitable on backends/configs that honor a tail call with a speculatable
instruction between it and the ret. X86 currently does not. The CodeGen
test suite has no coverage of this case (grep for `stack-protector` +
`musttail` produces no matches that mix this scenario).
