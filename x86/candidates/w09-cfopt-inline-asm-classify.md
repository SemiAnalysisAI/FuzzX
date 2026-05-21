## Candidate: classifyInstruction allows INLINEASM to "Skip" between frame setup and call

File: /home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Target/X86/X86CallFrameOptimization.cpp:283-356

### Reasoning
`classifyInstruction` returns `Exit` if `MI->isCall() || MI->mayStore()`, and `Exit`
if any def overlaps with previously used regs or uses the stack pointer. Otherwise
the default is `Skip`. However, an `INLINEASM`/`INLINEASM_BR` instruction that
contains `sideeffect` and reads/writes memory through implicit operands may not be
classified as `mayStore()` if no explicit memory operand is attached, since
`InlineAsm::Extra_MayStore` only sets `mayStore` when the asm flags actually request
it. An inline asm with no explicit Mem clobber but that effectively modifies the
stack argument region (e.g. via `asm volatile("..." ::: "memory")` with the right
constraints) will be classified as `Skip`, allowing it to live in the candidate
window between FrameSetup and the call. The pass then proceeds to convert MOVs to
PUSHes; if the inline asm depended on a particular SP-relative layout (a foot-gun
with strict-fp / x87 inline asm in particular), reordering breaks it.

A second concern: `classifyInstruction` doesn't check `MI->isInlineAsm()` directly,
nor does it stop on `MI->hasUnmodeledSideEffects()`. The general check for
"don't def any used reg" doesn't cover memory side-effects.

### Repro sketch
```
define void @foo(i32 %a, i32 %b) {
  call void @bar(i32 %a, i32 %b, i32 %a, i32 %b)
  ; Insert an inline asm between the stack mov setup and the call via a select
  ; or via a CMOV_GR8 expansion; the asm should have unmodeled side effects but
  ; no explicit memory operand.
  ret void
}
```

### Wrong outcome
The MOVs are converted to PUSHes that get reordered relative to the inline asm,
producing wrong arguments at the callee or clobbered SP-relative state used by the
asm. This is a "convert too aggressively" miscompile gated on unusual inputs.
