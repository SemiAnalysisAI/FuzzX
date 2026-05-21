# 014 — `X86TargetLowering::LowerRESET_FPENV` tags FLDENVm load with `MOStore`

Component: X86ISelLowering

## Source

`llvm/lib/Target/X86/X86ISelLowering.cpp:29415-29423` (`LowerRESET_FPENV`).

The pass builds a constant-pool buffer of FP-env bits and emits FLDENVm /
ldmxcsr (both **loads** from that constant pool) to install them. The MMO
attached is `MachineMemOperand::MOStore`. Compare `LowerGET_FPENV_MEM`
(line 29332-29335), which correctly re-flags the MMO to `MOLoad` before
attaching to FLDENVm.

A wrong-direction MMO flag misleads alias analysis: a later load/store
scheduler may treat this "store" as killing other loads or be reordered
around them in ways the source IR does not permit. The machine verifier
also flags this in some configurations.

## Fix

Change `MachineMemOperand::MOStore` to `MachineMemOperand::MOLoad` on the
constructed MMO (or re-flag inside `createSetFPEnvNodes` before attaching).

## Repro sketch

```ll
declare void @llvm.reset.fpenv()
define void @f(ptr %p) {
  %v0 = load i32, ptr %p
  call void @llvm.reset.fpenv()
  %v1 = load i32, ptr %p
  %s  = add i32 %v0, %v1
  store i32 %s, ptr %p
  ret void
}
```

Source-confirmed bug (wrong MMO flag). The observable miscompile depends on
which downstream pass next examines the MMO direction, which is target /
optimization-level sensitive — but the flag itself is clearly wrong.
