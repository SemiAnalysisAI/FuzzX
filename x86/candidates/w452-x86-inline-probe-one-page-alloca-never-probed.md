# X86 inline stack-probe emits a one-page allocation with **zero** probes

## Severity
HIGH — Same family as w450 (off-by-one in block-path tail handling), but
the smallest reproducer: a function whose computed `NumBytes` is exactly
one `StackProbeSize` (4096 bytes on x86-64 Linux). The generated prologue
contains a bare `subq $4096, %rsp` and no `movq $0, (%rsp)` probe at all.

This is the most distilled form of the bug. It is reported separately
from w450 because the trigger condition differs (one-page exact, not a
multiple) and a different code path is taken: the initial-probe `if`
condition at `X86FrameLowering.cpp:729` is **false**, so the unrolled
block emits literally zero probes for a full one-page alloca.

## Root cause
`emitStackProbeInlineGenericBlock` decides whether to emit the initial
sub+probe based on:

`llvm/lib/Target/X86/X86FrameLowering.cpp:729`
```cpp
if (StackProbeSize < Offset + AlignOffset) {
  // ... initial sub + probe
}
```

With the default `StackProbeSize = 4096` and `AlignOffset = 0` (no stack
realignment), `Offset == 4096` makes the predicate `4096 < 4096` —
**false**. The initial probe is skipped.

The subsequent loop guard also strictly less-than:

`llvm/lib/Target/X86/X86FrameLowering.cpp:752`
```cpp
while (CurrentOffset + StackProbeSize < Offset) {
```

`0 + 4096 < 4096` — false. Loop skipped.

The tail dispatch then runs with `ChunkSize = 4096`, which is neither
`SlotSize` (8) nor `< StackProbeSize`, falling into the `else` branch:

`llvm/lib/Target/X86/X86FrameLowering.cpp:780-783`
```cpp
} else {
  BuildStackAdjustment(MBB, MBBI, DL, -ChunkSize, /*InEpilogue=*/false)
      .setMIFlag(MachineInstr::FrameSetup);
}
// No need to probe the tail, it is smaller than a Page.
```

The comment is **wrong** here: `ChunkSize == StackProbeSize == 4096` is
not smaller than a page, it is exactly a page. The sub goes through
with no probe, and the comment's claim is silently invalid.

## Reproducer (default x86 `-O2`)
```ll
target triple = "x86_64-unknown-linux-gnu"

define void @one_page_no_probe() "probe-stack"="inline-asm" {
entry:
  ; alloca 4088 + 8 ret = 4096 total NumBytes (one StackProbeSize).
  %a = alloca [4088 x i8], align 16
  call void @use(ptr %a)
  call void asm sideeffect "", "~{rbx}"()  ; force one CSR push (rbx)
  ret void
}
declare void @use(ptr)
```

## Observed (llc 23.0.0git, `-O2 -mtriple=x86_64-unknown-linux-gnu`)
```asm
one_page_no_probe:
        pushq   %rbx
        .cfi_def_cfa_offset 16
        subq    $4096, %rsp                    # <-- the ENTIRE allocation
        .cfi_def_cfa_offset 4112               #     no probe at all.
        .cfi_offset %rbx, -16
        movq    %rsp, %rdi
        callq   use@PLT
        ...
```

The prologue is a single `subq $4096, %rsp` with no `movq $0, (%rsp)`. The
guarantee of `probe-stack="inline-asm"` — that every page in the
just-allocated frame has been touched before the function body runs — is
broken. The very next instruction (`movq %rsp, %rdi` for the use call,
followed by `callq use@PLT` which pushes the return address at
`(%rsp - 8)`) accesses a page that was never probed.

## Why this matters
The whole point of `probe-stack="inline-asm"` is to avoid relying on the
OS guard-page handler — e.g. inside kernel code, inside an alternate
signal stack, inside a sandboxed thread with a fixed-size stack, or any
context where a guard fault is unrecoverable. With this bug, a one-page
allocation in such a context will reliably crash on the first stack
access after the prologue, because the guard page beneath the just-
subtracted page is hit directly without any prior probe to extend mapped
stack pages.

## Expected
The initial `if` should use `<=`:
```cpp
if (StackProbeSize <= Offset + AlignOffset) {
  // initial sub + probe
}
```
Or the tail dispatch should emit a probe when `ChunkSize == StackProbeSize`
(symmetric to w450's required fix). One of these two corrections covers
both the one-page case (this report) and the multi-page exact-multiple
case (w450).

## Affected
`llvm/lib/Target/X86/X86FrameLowering.cpp:729` — strict `<` predicate
that skips the initial probe when `Offset == StackProbeSize`.

`llvm/lib/Target/X86/X86FrameLowering.cpp:780-783` — tail "no need to
probe" else-branch that fires whenever `ChunkSize` is exactly a page.
