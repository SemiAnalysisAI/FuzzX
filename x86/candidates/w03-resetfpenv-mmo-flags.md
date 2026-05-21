file: llvm/lib/Target/X86/X86ISelLowering.cpp:29415-29423

X86TargetLowering::LowerRESET_FPENV creates a constant-pool buffer of FP env
bits and uses `createSetFPEnvNodes` to load it into x87 (FLDENVm) and SSE
(ldmxcsr). However, the MachineMemOperand it constructs is given the flag
`MachineMemOperand::MOStore`, even though FLDENVm/ldmxcsr are *loads* from
that constant-pool address. Compare LowerGET_FPENV_MEM (line 29332-29335),
which correctly re-flags the MMO to MOLoad before handing it to FLDENVm.

A wrong-direction MMO can fool alias analysis: a later load-store scheduler
or post-RA scheduler may think the "store" to the constant pool kills other
loads (or be reordered around them) in ways the source IR does not allow.
That can manifest as missed instructions or wrong-ordering with respect to
adjacent FP env queries, and it confuses the machine verifier in some
configurations.

Candidate IR sketch:

  declare void @llvm.reset.fpenv()
  define void @f(ptr %p) {
    %v0 = load i32, ptr %p   ; aliasing probe
    call void @llvm.reset.fpenv()
    %v1 = load i32, ptr %p
    %s  = add i32 %v0, %v1
    store i32 %s, ptr %p
    ret void
  }

What's wrong:
  MachineMemOperand built with `MOStore` is attached to a LOAD instruction
  (FLDENVm and ldmxcsr). Should be `MOLoad`. Fix: change
  `MachineMemOperand::MOStore` to `MachineMemOperand::MOLoad` on line 29421,
  or re-flag inside createSetFPEnvNodes before attaching to FLDENVm.
