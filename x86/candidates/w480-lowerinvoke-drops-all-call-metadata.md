# LowerInvoke drops all metadata when replacing `invoke` with `call`

## File and root cause

`llvm/lib/Transforms/Utils/LowerInvoke.cpp:45-74` — `runImpl`.

```c++
static bool runImpl(Function &F) {
  bool Changed = false;
  for (BasicBlock &BB : F)
    if (InvokeInst *II = dyn_cast<InvokeInst>(BB.getTerminator())) {
      SmallVector<Value *, 16> CallArgs(II->args());
      SmallVector<OperandBundleDef, 1> OpBundles;
      II->getOperandBundlesAsDefs(OpBundles);
      // Insert a normal call instruction...
      CallInst *NewCall =
          CallInst::Create(II->getFunctionType(), II->getCalledOperand(),
                           CallArgs, OpBundles, "", II->getIterator());
      NewCall->takeName(II);
      NewCall->setCallingConv(II->getCallingConv());
      NewCall->setAttributes(II->getAttributes());
      NewCall->setDebugLoc(II->getDebugLoc());
      II->replaceAllUsesWith(NewCall);

      // Insert an unconditional branch to the normal destination.
      UncondBrInst::Create(II->getNormalDest(), II->getIterator());
      ...
```

Only `name`, `callingconv`, `attributes`, and `debugloc` are transferred from
the `invoke` to the new `call`. **No `copyMetadata()` call.** Every piece of
non-attribute metadata on the invoke is silently dropped:

* `!prof` (branch_weights on the invoke — interesting because the new
  unconditional branch can't carry these, but `!prof` on a call site is also
  used by VP / indirect-call promotion profiling)
* `!callees` (CFI / indirect-call promotion hint)
* `!annotation`
* `!nosanitize`
* `!noalias` / `!alias.scope`
* `!callback`
* `!srcloc`
* `!tbaa` (for memory-affecting calls)
* `!range` (return value range hint — InstCombine and SCEV consume this for
  call sites)

Additionally, the new `UncondBrInst` on line 63 is created with **no
`setDebugLoc`**, leaving it with an empty location in a function that has
debug info — which violates the verifier's "all instructions in a function
with `!dbg` must have a DebugLoc" expectation in optimized builds.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare i32 @bar(i32)
declare i32 @__gxx_personality_v0(...)

define i32 @f(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  %r = invoke i32 @bar(i32 %x) to label %ok unwind label %lpad,
       !prof !0, !callees !1, !annotation !2

ok:
  ret i32 %r

lpad:
  %l = landingpad { ptr, i32 } cleanup
  ret i32 0
}

declare i32 @baz(i32)
!0 = !{!"branch_weights", i32 100, i32 1}
!1 = !{ptr @bar, ptr @baz}
!2 = !{!"foo_call"}
```

### `opt -passes=lower-invoke -S` actual output

```llvm
define i32 @f(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  %r = call i32 @bar(i32 %x)         ; <-- NO !prof, NO !callees, NO !annotation
  br label %ok

ok:
  ret i32 %r
...
```

All three metadata attachments — `!prof`, `!callees`, `!annotation` — that
were on the source `invoke` are missing from the resulting `call`.

## Why this is a regression

* `!callees` is dropped → indirect-call promotion (ICP) loses the known-target
  set; subsequent CGSCC passes can no longer specialize. CFI hardening that
  used `!callees` to enumerate valid targets is silently weakened.
* `!prof` on the call site is consumed by PGO-driven inlining (`Inliner` uses
  it to inflate/deflate inline costs for cold/hot call sites). Dropping it
  flattens inliner heuristics.
* `!annotation` is contractually preserved by all transforms per LangRef
  ("should be preserved by optimization passes"); LowerInvoke breaks the
  contract.
* `!noalias` / `!alias.scope` loss is an alias-analysis regression for any
  downstream BasicAA / MemorySSA query about the call's memory effects.

In x86 `-O2`, `LowerInvokePass` runs when the target/backend strips EH (e.g.,
`-fno-exceptions` paths, embedded targets, certain sanitizer modes). The bug
is reproducible by directly invoking `opt -passes=lower-invoke -S`.

## Fix sketch

After `NewCall->setDebugLoc(...)`, add:

```c++
NewCall->copyMetadata(*II);   // copies all non-attribute MD verbatim
```

The verifier will reject `!dbg` on the new call only if the surrounding
function has debug info; `copyMetadata` already replays `!dbg` correctly via
the metadata-clone path.

For the unconditional branch on line 63, set its DebugLoc from the invoke:

```c++
auto *Br = UncondBrInst::Create(II->getNormalDest(), II->getIterator());
Br->setDebugLoc(II->getDebugLoc());
```
