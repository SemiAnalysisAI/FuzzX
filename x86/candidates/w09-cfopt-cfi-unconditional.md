## Candidate: X86CallFrameOptimization emits DW_CFA_adjust_cfa_offset unconditionally

File: /home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Target/X86/X86CallFrameOptimization.cpp:576-579

### Reasoning
In `adjustCallSequence`, after replacing a MOV-to-stack with a PUSH, the pass emits
`MCCFIInstruction::createAdjustCfaOffset(nullptr, SlotSize)` whenever `!TFL->hasFP(MF)`.
The guard only checks for the presence of an FP, NOT `needsDwarfCFI(MF)` or
`MF.needsFrameMoves()`. Other call sites of `BuildCFI` in X86FrameLowering universally
gate themselves on `NeedsDwarfCFI` (e.g. lines 1806, 1884). The unconditional emission
here produces `.cfi_adjust_cfa_offset` directives for functions that have no unwind
table requested (no `uwtable`, no SEH, no EH) and for non-DWARF unwinding ABIs that
aren't already filtered out by `isLegal` (only Win64 is rejected at line 155). For
SEH-using targets such as i686-windows-msvc or x86_64-windows-gnu (mingw, which isn't
matched by `isTargetWin64()` checks for SEH-only filtering), this introduces stray
DWARF CFI directives in `.eh_frame`/`.debug_frame` even when the rest of the prologue
emits only WinCFI/SEH directives, producing inconsistent unwind info.

### Repro sketch
```
; i686-pc-linux with -fno-asynchronous-unwind-tables and -fno-unwind-tables
; A function with a call passing >=2 stack args. Currently still gets
; .cfi_adjust_cfa_offset directives despite no CFI being requested.
define void @foo(i32 %a, i32 %b) nounwind {
  call void @bar(i32 %a, i32 %b)
  ret void
}
declare void @bar(i32, i32)
```
Compile `llc -mtriple=i686-pc-linux -no-x86-call-frame-opt=false`. The output gets
`.cfi_adjust_cfa_offset 4` after each push despite `nounwind` and no `uwtable`.

### Wrong outcome
Spurious `.cfi_*` directives in the asm/object output for functions that opted out of
unwind info; for SEH-only ABIs, a mixture of DWARF and SEH directives in the same
function results in broken `.eh_frame` and/or assembler errors.
