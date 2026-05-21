# w96: GVNSink merges calls whose bundle operands disagree (PHIing semantic state)

## Affected pass
`llvm/lib/Transforms/Scalar/GVNSink.cpp` together with
`llvm/lib/Transforms/Utils/Local.cpp::canReplaceOperandWithVariable`.

## Root cause (companion to w96-gvnsink-deopt-bundle-operand-phi)

This is the same underlying root cause as the deopt-operand candidate, but
framed around `convergencectrl`-like bundles and around the fact that
`hasSameSpecialState` (which `isSameOperationAs` calls into) only checks
`hasIdenticalOperandBundleSchema` (tag + operand count) and *not* operand
values:

```cpp
// Instruction.cpp:937
if (const CallInst *CI = dyn_cast<CallInst>(I1))
  return CI->isTailCall() == cast<CallInst>(I2)->isTailCall() &&
         CI->getCallingConv() == cast<CallInst>(I2)->getCallingConv() &&
         CheckAttrsSame(CI, cast<CallInst>(I2)) &&
         CI->hasIdenticalOperandBundleSchema(*cast<CallInst>(I2));
```

`hasIdenticalOperandBundleSchema` (InstrTypes.h:2150) only compares the
per-bundle (tagID, # operands) tuples, never the operand identity. So
two calls with `["deopt"(i32 %a)]` and `["deopt"(i32 %b)]` register as
"same operation".

GVNSink then enters the per-operand PHI loop (GVNSink.cpp:690) and asks
`canReplaceOperandWithVariable` whether each operand may become a PHI.
For *non-constant* operands, `canReplaceOperandWithVariable` returns true
on its early-exit line 3916, never reaching the bundle-operand guard at
line 3932. So a non-constant bundle operand (the common case for deopt /
gc-live / convergencectrl / funclet operands referencing SSA values from
the surrounding scope) is freely PHI-fied across paths.

In the deopt case (see the companion candidate) this corrupts the
deoptimization stackmap. In the convergencectrl case it would also be a
miscompile, but the existing token-type guard at line 3902 of
`canReplaceOperandWithVariable` rejects token-typed operands, so
`convergencectrl` bundles are accidentally safe.

The check is therefore wrong-for-the-right-reason on tokens and outright
wrong on i32/ptr deopt operands.

## Reduced reproducer (same as w96-gvnsink-deopt-bundle-operand-phi)

`/tmp/w96-control.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @callee(i32) #0
declare void @use(i32)

define void @test(i1 %c, i32 %x, i32 %a, i32 %b) gc "statepoint-example" {
entry:
  br i1 %c, label %ba, label %bb

ba:
  call void @callee(i32 %x) [ "deopt"(i32 %a) ]
  call void @use(i32 %a)
  br label %end

bb:
  call void @callee(i32 %x) [ "deopt"(i32 %b) ]
  call void @use(i32 %b)
  br label %end

end:
  ret void
}

attributes #0 = { nounwind }
```

## opt diff

```
$ build/llvm-fuzzer/bin/opt -passes=gvn-sink -S /tmp/w96-control.ll
```

After:

```llvm
end:
  %b.sink1 = phi i32 [ %b, %bb ], [ %a, %ba ]
  %b.sink  = phi i32 [ %b, %bb ], [ %a, %ba ]
  call void @callee(i32 %x) [ "deopt"(i32 %b.sink1) ]   ; <-- deopt bundle operand is now a PHI
  call void @use(i32 %b.sink)
  ret void
```

(The IR verifier accepts this output, so the corruption flows to the
backend / runtime.)

## Fix

`hasSameSpecialState` should additionally require that bundle operands
match (or, equivalently, `canReplaceOperandWithVariable` should hoist its
`if (CB.isBundleOperand(OpIdx)) return false;` out of the `Constant`
branch). Either change makes the per-operand PHI loop in GVNSink refuse
to sink calls whose bundle operands disagree across paths, regardless of
whether the operands are constants or runtime SSA values.

Equivalent care is needed in any other transform that uses these helpers
to manufacture PHIs across calls -- notably `SimplifyCFG`'s
`sink-common-insts` path, `JumpThreading`'s common-tail sinking, and
`LoopUnswitch`'s instruction sinking helper. They all read through the
same `canReplaceOperandWithVariable` predicate.
