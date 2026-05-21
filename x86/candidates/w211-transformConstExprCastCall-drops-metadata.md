# w210 — `transformConstExprCastCall` drops all metadata except `MD_prof`

## Where
`llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:5223`

```cpp
// Preserve prof metadata if any.
NewCall->copyMetadata(*Caller, {LLVMContext::MD_prof});
```

## What's wrong
When InstCombine rewrites a call through a bitcast of a function (`call (bitcast @foo to T*)(args)`) into a direct call with cast args, it allocates a new `CallBase` and explicitly copies only `MD_prof` metadata. **Every other metadata kind attached to the original call is silently dropped**, including (non-exhaustive):

- `!callees` — indirect-call promotion hints
- `!annotation`
- `!nosanitize`
- `!noalias`, `!alias.scope` (when this call is itself a memory call)
- `!srcloc` (inline asm)
- `!type` (CFI / WPD)

The new call does receive `!dbg` indirectly via `Builder` debug-loc propagation in some cases, but only when the `IRBuilder`'s current debug location happens to be set; the `setDebugLoc(Caller->getDebugLoc())` at line 5231 is applied only to the inserted return-value bitcast, NOT to `NewCall` itself.

The matching `setMetadata` block for `MD_prof` is also unconditional — even if the original call had no `!prof`, the call to `copyMetadata` is a no-op there, but it leaves stale data when other passes inject metadata.

## Severity / class
Loss of optimization metadata, plus potential debug-info regression. The most concerning case is `!callees`/`!callback`, which guide post-instcombine IPO; dropping them silently can change optimization decisions downstream. Not a miscompile of program behavior, but a metadata regression hidden inside a heavily-used fold path.

## Reproducer

```ll
; opt -passes=instcombine -S
declare void @callee(ptr)

define void @test(ptr %p) {
  call void @callee(ptr %p), !annotation !0, !callees !1
  ret void
}

declare void @other()

!0 = !{!"annotation-A"}
!1 = !{ptr @callee, ptr @other}
```

After instcombine, the call still references `@callee` directly (no cast involved here), but for cases that go through `transformConstExprCastCall` (e.g. mismatched-prototype indirect calls), `!annotation` and `!callees` are dropped on the rewritten call.

## Notes
- Suggested fix: copy `Caller->getAllMetadata()` then set or strip kinds that aren't safe on the new call type. Or use `IRBuilder::SetCurrentDebugLocation(Caller->getDebugLoc())` before `Builder.CreateCall(...)` to at least preserve debug info.
- Likely affects every front-end that emits both `!callees` and bitcast-call patterns (LTO, ThinLTO IPO promotion).
