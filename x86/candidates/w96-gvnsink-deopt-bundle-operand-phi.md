# w96: GVNSink turns deopt bundle operand into a PHI, breaking statepoint reconstruction

## Affected pass
`llvm/lib/Transforms/Scalar/GVNSink.cpp` + `llvm/lib/Transforms/Utils/Local.cpp::canReplaceOperandWithVariable`

## Root cause

`GVNSink::analyzeInstructionForSinking` (around line 690 of `GVNSink.cpp`)
walks each operand of the candidate instructions and, when the values
differ across predecessors, queries `canReplaceOperandWithVariable(I0, OpNum)`
to decide whether it is safe to replace that operand with a PHI:

```cpp
for (unsigned OpNum = 0, E = I0->getNumOperands(); OpNum != E; ++OpNum) {
  ModelledPHI PHI(NewInsts, OpNum, ActivePreds);
  if (PHI.areAllIncomingValuesSame())
    continue;
  if (!canReplaceOperandWithVariable(I0, OpNum))
    return std::nullopt;
  ...
```

`canReplaceOperandWithVariable` in `Local.cpp` checks bundle operands, but
**only inside the `if (isa<Constant, InlineAsm>(Op))` branch** at line 3916.
For *non-constant* bundle operands the function early-exits TRUE on line
3917, so GVNSink is happily allowed to introduce a PHI for any non-constant
deopt/funclet/gc-live bundle operand:

```cpp
if (!isa<Constant, InlineAsm>(Op))
  return true;             // <-- non-const bundle operands skip the
                           //     `if (CB.isBundleOperand(OpIdx)) return false;`
                           //     check below.
```

A deopt bundle operand is required to be a *path-specific* description of
the abstract value that the runtime deoptimizer (or VM) will use to
reconstruct the source-language state at the abstract program point. After
sinking, the bundle operand becomes a PHI whose incoming values came from
different branches: the recorded "deopt value" is now a runtime select, not
a snapshot of the source-language variable on that path.

`hasSameSpecialState` (Instruction.cpp:937) only requires
`hasIdenticalOperandBundleSchema` (tag + operand count), so bundles with
identical tags but different operand *values* pass that gate.

## Reduced reproducer

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

Before sink: each branch records its own deopt value (`%a` on the `ba`
path, `%b` on the `bb` path).

After sink:

```llvm
end:
  %b.sink1 = phi i32 [ %b, %bb ], [ %a, %ba ]
  %b.sink  = phi i32 [ %b, %bb ], [ %a, %ba ]
  call void @callee(i32 %x) [ "deopt"(i32 %b.sink1) ]
  call void @use(i32 %b.sink)
  ret void
```

The deopt operand is now `%b.sink1`, a PHI. The bundle still says it
records `i32`, but the value the runtime sees is a runtime-selected scalar
rather than the pair `(ba -> %a, bb -> %b)` that the deopt machinery is
supposed to materialize.

The IR verifier accepts this without complaint, so the miscompile flows
straight through to lowering. For real users (e.g. WebKit / V8 / RyuJIT
front ends that emit deopt bundles for tier-down) the deopt frame is now
corrupted: the runtime will reconstruct using the wrong slot value for at
least one of the two original paths.

## Fix sketch

`canReplaceOperandWithVariable` should move the
`if (CB.isBundleOperand(OpIdx)) return false;` check *out* of the
`Constant` early-exit branch. Bundle operand semantics are not contingent
on the operand being a constant; the rule that "constant bundle operands
may need to retain their constant-ness for correctness" is even weaker
than the actual invariant that "*all* bundle operands describe path-
specific deopt / funclet / gc state and must not be merged across paths
into a PHI".

Equivalently, GVNSink (and any other PHI-introducing pass that uses
`canReplaceOperandWithVariable`) should refuse to sink calls that have any
operand bundle whose tag is in the "semantic" set (deopt, gc-live,
funclet, convergencectrl) when the bundle operands disagree.
