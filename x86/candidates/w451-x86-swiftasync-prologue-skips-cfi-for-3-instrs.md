# X86 SwiftAsync prologue emits 3 stack-changing instructions with no DWARF CFI

## Severity
MEDIUM — async-unwind safety bug. If a profiler, signal handler, or other
asynchronous unwinder samples a stack inside these three instructions of a
`swifttailcc` (or any `swiftasync`-attributed) function's prologue, the
unwinder will compute a CFA that is wrong by 8 or 16 bytes and follow the
return-address chain incorrectly.

## Root cause
`X86FrameLowering::emitPrologue` handles the SwiftAsync context after pushing
%rbp. The sequence emitted is (with no intervening DWARF CFI):

`llvm/lib/Target/X86/X86FrameLowering.cpp:1801-1822` — `pushq %rbp` plus the
two `BuildCFI` calls that update `.cfi_def_cfa_offset` and the rbp `.cfi_offset`.
After this, the CFI state correctly says CFA = `%rsp + 16`.

`llvm/lib/Target/X86/X86FrameLowering.cpp:1832-1873` — when the function has
SwiftAsync context, three more stack-mutating instructions are emitted:

```cpp
if (X86FI->hasSwiftAsyncContext()) {
  ...
  if (Attrs.hasAttrSomewhere(Attribute::SwiftAsync)) {
    MBB.addLiveIn(X86::R14);
    BuildMI(MBB, MBBI, DL, TII.get(X86::PUSH64r))   // pushq %r14
        .addReg(X86::R14)
        .setMIFlag(MachineInstr::FrameSetup);
  } else {
    BuildMI(MBB, MBBI, DL, TII.get(X86::PUSH64i32)) // pushq $0
        .addImm(0)
        .setMIFlag(MachineInstr::FrameSetup);
  }

  if (NeedsWinCFI) { ... SEH_PushReg ... }          // SEH only — DWARF nothing

  BuildMI(MBB, MBBI, DL, TII.get(X86::LEA64r), FramePtr)  // lea  8(%rsp), %rbp
      .addUse(X86::RSP).addImm(1).addUse(X86::NoRegister)
      .addImm(8).addUse(X86::NoRegister)
      .setMIFlag(MachineInstr::FrameSetup);
  BuildMI(MBB, MBBI, DL, TII.get(X86::SUB64ri32), X86::RSP) // subq $8, %rsp
      .addUse(X86::RSP).addImm(8).setMIFlag(MachineInstr::FrameSetup);
}
```

After all three of these, the next CFI directive comes from
`X86FrameLowering.cpp:1902-1905` which emits `createDefCfaRegister(rbp)`.
No `.cfi_adjust_cfa_offset` is issued for the extra `pushq %r14` (-8) or
the extra `subq $8, %rsp` (-8), and no `.cfi_offset %r14, ...` is emitted
for the pushed async context register.

Because the only `createDefCfaRegister` re-anchors CFA at `%rbp + 16`,
which happens to land back on the correct address, the *final* CFA state is
self-consistent. But during the three intermediate instructions, an
asynchronous unwinder reading the CFI sees a stale `CFA = %rsp + 16`,
which under-shoots the true CFA by 8 bytes (after pushq r14) and then
16 bytes (after subq $8, %rsp). The unwinder consequently misidentifies
the return address slot and reads garbage.

## Reproducer (default x86 `-O2`)
```ll
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr swiftasync, ptr)

define swifttailcc void @swiftasync_realign(ptr swiftasync %ctx) "stackrealign" {
  %a = alloca [128 x i8], align 64
  call void @use(ptr swiftasync %ctx, ptr %a)
  ret void
}
```

## Observed (llc 23.0.0git, `-O2 -mtriple=x86_64-unknown-linux-gnu`)
```asm
swiftasync_realign:
        btsq    $60, %rbp
        pushq   %rbp
        .cfi_def_cfa_offset 16
        .cfi_offset %rbp, -16
        pushq   %r14                  # <-- no .cfi_adjust_cfa_offset 8
        leaq    8(%rsp), %rbp         # <-- still no CFI catch-up
        subq    $8, %rsp              # <-- no .cfi_adjust_cfa_offset 8
        .cfi_def_cfa_register %rbp    # <-- re-anchors; final state correct
        andq    $-64, %rsp
        subq    $192, %rsp
        ...
```

Three SP-changing instructions sit between the only `.cfi_def_cfa_offset 16`
and the `.cfi_def_cfa_register %rbp`. Async-unwind through any of them
gets CFA wrong by 8 or 16 bytes.

There is also no `.cfi_offset %r14, ...` for the pushed async context,
even though %r14 is conventionally part of the swift async link.

## Expected
Either:
1. Emit `.cfi_adjust_cfa_offset 8` after the `pushq %r14` (and a matching
   `.cfi_offset %r14, -24` when applicable), then again after the
   `subq $8, %rsp`. Followed by the existing `.cfi_def_cfa_register %rbp`
   that re-anchors at rbp.
2. Or, move the `.cfi_def_cfa_register %rbp` to immediately after `pushq %r14`,
   and emit the `lea`+`subq` in the order that keeps CFA at `%rbp + N` for
   a known constant N throughout.

## Affected
`llvm/lib/Target/X86/X86FrameLowering.cpp:1844-1872` — the SwiftAsync
context push + frame-pointer LEA + RSP subq sequence emits **no** DWARF
CFI updates, while the surrounding code paths for non-swift-async cases do.
