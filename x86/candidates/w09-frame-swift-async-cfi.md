## Candidate: SwiftAsyncContext push missing CFI offset update before LEA/SUB

File: /home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Target/X86/X86FrameLowering.cpp:1832-1872

### Reasoning
When a function has `X86FI->hasSwiftAsyncContext()`, the prologue does this sequence
(after the saved-FP push at line 1801 has set CFA offset = 16):

1. PUSH r14 (or `pushq $0`) - line 1844/1850 - pushes the async context
2. SEH_PushReg (only if NeedsWinCFI) - line 1857
3. LEA rbp, [rsp+8] - line 1862 - sets FP to point at the saved-FP slot
4. SUB rsp, 8 - line 1869 - reserves the async-context slot below FP

When `NeedsDwarfCFI` is true, the FP-push at line 1801 emits a CFI rule at line 1810:
`createDefCfaOffset(-2 * stackGrowth + ...)` (i.e. CFA = SP + 16). After the PUSH at
line 1844, CFA should be SP + 24, but NO CFI directive is emitted for the swift-async
push. The next CFI update happens only at line 1902 (`createDefCfaRegister(FP)`),
which is emitted AFTER both the LEA and the SUB. Between the swift-context PUSH
and the DefCfaRegister, the CFA offset rule is wrong by 8 bytes.

Furthermore the SUB at line 1869 occurs after the FP is established but the CFA
rule already references FP, so unwinding hitting an exception during the SUB
sequence still works for the CFA-via-FP rule. The window of incorrect CFI is
between the PUSH (line 1844/1850) and the LEA (line 1862): in that window the
unwinder will compute `CFA = RSP + 16` but the real CFA is `RSP + 24`, producing
return-address recovery to the wrong slot.

### Repro sketch
```
; X86 swiftasync prologue
define swiftcc void @foo(ptr swiftasync %ctx) "frame-pointer"="all" {
  call void asm sideeffect "int3", ""()  ; force signal between PUSH and LEA
  ret void
}
```
Build with `llc -mtriple=x86_64-apple-macosx -O0` and inspect `.eh_frame`; the FDE
between the asynccontext-push offset and the FP-establishing LEA reports the wrong
CFA offset.

### Wrong outcome
Backtraces taken in the prologue window report the wrong frame, breaking debuggers
and exception unwinding for Swift async functions.
