# X86 inline stack-probe skips a full page when the tail equals StackProbeSize

## Severity
HIGH — Generated code can skip a guard page, breaking the stack-probe contract.
Any access into the unprobed bottom page (e.g. the very first instruction of
the function body that touches the alloca) crosses the OS guard boundary by
two pages instead of one, producing an unrecoverable SIGSEGV rather than the
expected guard-page handler-driven extension.

## Root cause
`X86FrameLowering::emitStackProbeInlineGenericBlock` decides how to handle the
"tail" of an inline `probe-stack="inline-asm"` allocation after its
unrolled probe loop. The dispatch is:

`llvm/lib/Target/X86/X86FrameLowering.cpp:771-783`
```cpp
uint64_t ChunkSize = Offset - CurrentOffset;
if (ChunkSize == SlotSize) {
  // Use push for slot sized adjustments as a size optimization...
  unsigned Reg = Is64Bit ? X86::RAX : X86::EAX;
  unsigned Opc = Is64Bit ? X86::PUSH64r : X86::PUSH32r;
  BuildMI(MBB, MBBI, DL, TII.get(Opc))
      .addReg(Reg, RegState::Undef)
      .setMIFlag(MachineInstr::FrameSetup);
} else {
  BuildStackAdjustment(MBB, MBBI, DL, -ChunkSize, /*InEpilogue=*/false)
      .setMIFlag(MachineInstr::FrameSetup);
}
// No need to probe the tail, it is smaller than a Page.
```

The "No need to probe" comment is only valid when `ChunkSize < StackProbeSize`.
The loop condition that produced `CurrentOffset` is:

`X86FrameLowering.cpp:752`
```cpp
while (CurrentOffset + StackProbeSize < Offset) {
```

This uses a strict `<`, so the loop exits *one chunk early* whenever `Offset`
is an exact multiple of `StackProbeSize`. In that case
`ChunkSize == StackProbeSize` (4096), the `else` branch fires, and the tail
adjustment is performed by a plain `subq` with **no** accompanying
`movq $0, (%rsp)` probe.

The result is that the final 4 KiB page of the allocation is decremented
through without being touched. The next stack access skips over the OS guard
page boundary instead of triggering the expected guard-page fault.

## Reproducer (default x86 `-O2`)
```ll
target triple = "x86_64-unknown-linux-gnu"

define void @one_csr_probe() "probe-stack"="inline-asm" {
entry:
  %a = alloca [16384 x i8], align 16
  call void @use(ptr %a)
  call void asm sideeffect "", "~{rbx}"()      ; force one CSR push
  ret void
}
declare void @use(ptr)
```

Frame layout for the prologue:
- `pushq %rbx` (8 B saved CSR)
- return address already on stack (8 B)
- alloca 16384 B (16-byte aligned)

So `NumBytes = 16384 == 4 * StackProbeSize`. The block-path is taken because
`16384 == ProbeChunk == 8 * 4096`, satisfying `Offset > ProbeChunk` is FALSE,
so `emitStackProbeInlineGenericBlock` runs. Trace:
- initial `if (StackProbeSize < Offset + 0)` -> sub+probe, `CurrentOffset=4096`
- loop iter 1: `8192 < 16384` -> sub+probe, `CurrentOffset=8192`
- loop iter 2: `12288 < 16384` -> sub+probe, `CurrentOffset=12288`
- loop test: `16384 < 16384` is **false**, exit
- `ChunkSize = 16384 - 12288 = 4096`, not `SlotSize`, emit plain `subq $4096`

## Observed (llc 23.0.0git, `-O2 -mtriple=x86_64-unknown-linux-gnu`)
```asm
one_csr_probe:
        pushq   %rbx
        .cfi_def_cfa_offset 16
        subq    $4096, %rsp
        .cfi_adjust_cfa_offset 4096
        movq    $0, (%rsp)              # probe page 1
        subq    $4096, %rsp
        .cfi_adjust_cfa_offset 4096
        movq    $0, (%rsp)              # probe page 2
        subq    $4096, %rsp
        .cfi_adjust_cfa_offset 4096
        movq    $0, (%rsp)              # probe page 3
        subq    $4096, %rsp             # <-- TAIL SUB, no probe!
        .cfi_def_cfa_offset 16400
        .cfi_offset %rbx, -16
        movq    %rsp, %rdi
        callq   use@PLT
        ...
```

Only three `movq $0, (%rsp)` probes are emitted, but four `subq $4096`
adjustments execute. The 4th page (covering `[%rsp, %rsp+4095]` after the
final sub) is never touched. The subsequent `callq use@PLT` writes the
return address at `(%rsp - 8)` — which lies in a fifth page that has never
been probed at all, breaking the one-page-at-a-time guard-page contract.

Same pattern reproduces with `Offset = 8192` (one probe + one unprobed sub)
and any other multiple of `StackProbeSize` not handled by the `SlotSize`
push optimization. With `returns_twice` callees this triggers reliably
because the live setjmp return value forces exactly one CSR push:

```ll
declare i32 @setjmp(ptr) returns_twice
@buf = global [200 x i64] zeroinitializer
define i32 @sj_probe() "probe-stack"="inline-asm" {
  %a = alloca [16384 x i8], align 16
  %v = call i32 @setjmp(ptr @buf) returns_twice
  call void @use()
  ret i32 %v
}
declare void @use()
```

## Expected
`while (CurrentOffset + StackProbeSize <= Offset)` (i.e. `<=`) so the tail
is `< StackProbeSize` and is genuinely safe to skip the probe on, OR
explicitly emit `movq $0, (%rsp)` after the tail sub when the tail equals
a full page.

## Affected
`llvm/lib/Target/X86/X86FrameLowering.cpp:752` (off-by-one in loop bound)
and `llvm/lib/Target/X86/X86FrameLowering.cpp:771-786` (tail dispatch that
assumes `ChunkSize < StackProbeSize` without verifying).
