# w380: Inliner drops call-site return attributes/metadata when callee has `returned` parameter

## Summary

When inlining a callee whose return value is its `returned`-attributed parameter
(or is a call whose result `simplifyInstruction` replaces with the cloned
argument), the **call-site return attributes** (`range`, `nonnull`, `align`,
`dereferenceable`, `dereferenceable_or_null`, `noalias`, `noundef`, `nofpclass`)
and the **`!range` return metadata** on the original call are silently dropped.

The information is lost because `AddReturnAttributes` looks for a cloned
`CallBase` to re-attach the call-site attributes to. When the cloned return
operand is no longer a `CallBase` (because (a) the callee `ret`'s the
parameter directly, or (b) `simplifyInstruction` during cloning replaced the
inner `returned` call's result with the cloned argument), the function bails out
at the `dyn_cast_or_null<CallBase>` and the attributes are never transferred to
the value that ends up in the caller (which is just the actual argument value).

Result: missed optimizations in `instcombine`/`gvn`/`SCEV`/etc., and (depending
on the downstream pipeline) extra real instructions in the x86 -O2 output.

## Source

`llvm/lib/Transforms/Utils/InlineFunction.cpp:1597-1726`

```
1606    for (auto &BB : *CalledFunction) {
1607      auto *RI = dyn_cast<ReturnInst>(BB.getTerminator());
1608      if (!RI || !isa<CallBase>(RI->getOperand(0)))
1609        continue;                                   // <-- case (a): ret Argument
1610      auto *RetVal = cast<CallBase>(RI->getOperand(0));
1611      ...
1614      auto *NewRetVal = dyn_cast_or_null<CallBase>(VMap.lookup(RetVal));
1615      if (!NewRetVal)
1616        continue;                                   // <-- case (b): cloned call
1617                                                    //     simplified to non-call
```

The replacement that triggers case (b) is in
`llvm/lib/Transforms/Utils/CloneFunction.cpp:867-878`:

```
867      if (Value *V = simplifyInstruction(NewI, DL)) {
868        NewI->replaceAllUsesWith(V);
869        if (isInstructionTriviallyDead(NewI)) NewI->eraseFromParent();
870        else VMap[&I] = NewI;
```

`simplifyInstruction` on a `call ... @inner(i32 returned %x)` returns the
`returned` operand `%x`, so the cloned `ret`'s operand is RAUW'd to `%x`.
The cloned call is kept (it has side effects via the external decl) but the
mapping `RetVal -> cloned call` is left in `VMap`, while the actual returned
value (after RAUW) is `%x`. The check at line 1620
`InlinedFunctionInfo.isSimplified(RetVal, NewRetVal)` does NOT fire for this
shape because the VMap entry still points at the cloned call.

For case (a) the bail-out is even more direct: the return operand is an
`Argument`, so `!isa<CallBase>` at line 1608 immediately `continue`s without
looking at any of the call-site attributes.

There is no fallback to propagate the attributes to the actual returned
SSA value (e.g., by emitting an `llvm.assume`, attaching `!range` to a load,
or marking the cloned `inner` call with the intersection of attributes — which
would actually be valid because `returned %x` guarantees inner's result *is*
the argument).

## Reproducer

`/tmp/inline-bugs/w380.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(i32)

define i32 @callee(i32 returned %x) {
entry:
  call void @use(i32 %x)
  ret i32 %x
}

define i1 @caller(i32 %x) {
entry:
  %r = call range(i32 0, 100) i32 @callee(i32 %x)
  %c = icmp ult i32 %r, 200
  ret i1 %c
}
```

### Diff `opt -passes='inline,instcombine' -S` vs `opt -passes='instcombine' -S`

With `inline,instcombine` (the bug):

```
define i1 @caller(i32 %x) {
entry:
  call void @use(i32 %x)
  %c = icmp ult i32 %x, 200       ; <-- icmp NOT folded
  ret i1 %c
}
```

Without inline, the same callsite shape on an external decl folds:

```
define i1 @caller_ni(i32 %x) {
entry:
  %r = call range(i32 0, 100) i32 @ext(i32 %x)
  ret i1 true                     ; <-- folded
}
```

## Other attributes affected (same shape, separate one-shot tests)

| Attribute on call site | Folded without inline? | Folded after `inline,instcombine`? |
|---|---|---|
| `range(i32 0,100)` + `icmp ult %r, 200` | yes (`ret i1 true`) | no |
| `!range !{0,100}` MD + `icmp ult %r, 200` | yes | no |
| `nonnull` + `icmp eq %r, null` | yes (`ret i1 false`) | no |
| `align 64` + `(ptrtoint %r) & 63` | yes (`ret i64 0`) | no |
| `dereferenceable(8) align 16` + `(ptrtoint %r) & 15` | yes | no |
| `nofpclass(nan)` + `fcmp uno %r, 0.0` | yes (`ret i1 false`) | no |
| Case (b): `ret call i32 @inner(i32 returned %x)` | yes | no |

All variants tested on opt `LLVM 23.0.0git` from
`/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`.

## x86 -O2 visibility

`-O2` recovers some of these losses via late IPO (function-attrs deduction
re-infers `range(i32 0, 21474837)` on the *whole function* and then folds the
constant `udiv`). However:

- The CGSCC inliner pipeline runs `inline,instcombine,...` rounds before that
  recovery; **other intermediate optimizations that depend on the call-site
  attribute do not get the info during their window** (e.g. early SROA,
  EarlyCSE-with-AA, NewGVN, JumpThreading, etc.).
- Functions whose returned-arg path is more complex (multiple uses of `%r` with
  different attributes per use) can never be reconstructed by function-attrs
  because the per-call-site distinction has been erased.
- For the `nofpclass` and `noundef` cases, recovery via function-attrs is
  weaker (they have stricter inference conditions).

## Fix sketch

In `AddReturnAttributes`, when the cloned return value is the cloned actual
argument (i.e., `VMap.lookup(RetVal)` is the cloned `Argument` value, or the
return operand at the IR level is an `Argument` whose `Returned` is set):
- Walk to the value that will replace the call in the caller (= the actual
  argument operand of `CB`), and attach an `llvm.assume` carrying the
  poison-generating attributes; or
- Attach a fresh `CallBase` wrapping the actual argument (an `@llvm.invariant`
  noop) — but this defeats the point of `returned`.

The cleanest fix is to recognize the `returned` case explicitly and emit
`llvm.assume(@llvm.assume.cond(<actualArg>, range/nonnull/...))` so downstream
passes recover the info, matching what `PreserveAlignmentAssumptions`
(line 1731) already does for `align`-on-byval args.

## Confidence

High — clear root cause, multiple attribute kinds reproduced, contrast against
the non-inlined version is direct.
