# X86ExpandPseudo: CALL_RVMARKER uses SysV (CallingConv::C) preserved mask for Windows ObjC runtime call

File: llvm/lib/Target/X86/X86ExpandPseudo.cpp:241-261

```
241    // Emit marker "movq %rax, %rdi".  %rdi is not callee-saved, so it cannot be
242    // live across the earlier call. The call to the ObjC runtime function returns
243    // the first argument, so the value of %rax is unchanged after the ObjC
244    // runtime call. On Windows targets, the runtime call follows the regular
245    // x64 calling convention and expects the first argument in %rcx.
246    auto TargetReg = STI->getTargetTriple().isOSWindows() ? X86::RCX : X86::RDI;
247    auto *Marker = BuildMI(MBB, MBBI, MI.getDebugLoc(), TII->get(X86::MOV64rr))
...
251    // Emit call to ObjC runtime.
252    const uint32_t *RegMask =
253        TRI->getCallPreservedMask(*MBB.getParent(), CallingConv::C);     // <-- BUG
254    MachineInstr *RtCall =
255        BuildMI(MBB, MBBI, MI.getDebugLoc(), TII->get(X86::CALL64pcrel32))
256            .addGlobalAddress(MI.getOperand(0).getGlobal(), 0, 0)
257            .addRegMask(RegMask)
258            ...
```

Reasoning: The code carefully switches the marker register (RDI vs RCX)
based on whether the target is Windows, indicating it knows the runtime
call uses Win64 CC on Windows. But the preserved-register mask used to
describe what the runtime call clobbers is always
`getCallPreservedMask(MF, CallingConv::C)`, i.e. SysV. Win64 preserves a
strictly larger set (RSI, RDI, R12-R15, XMM6-XMM15) than SysV, so on
Win64 the resulting machine instruction over-specifies clobbers (no
miscompile) — BUT the inverse also matters: on SysV, the
SysV-C mask is correct. The actual codegen risk is when the X86 backend
is asked to build for `x86_64-pc-windows-*` and emits CALL_RVMARKER:
the use of SysV mask claims XMM6-XMM15 are clobbered when in reality
the Win64 ABI preserves them, potentially forcing unnecessary spills
in the caller. (Pessimisation, but the comment shows the author already
went out of their way to distinguish Win64; the mismatched mask is
inconsistent with that intent.)

This is mostly a perf / ABI-consistency concern, not a miscompile in
the SysV direction (over-clobbering is conservatively correct). I'm
flagging it because the existence of CALL_RVMARKER on Windows is
explicitly anticipated by line 246, and a sibling clang-driven test
that lowers a `clang.arc.attachedcall` bundle on
`x86_64-pc-windows-msvc` will exhibit this asymmetry in the regmask.

Repro sketch:
- IR with `call ... [ "clang.arc.attachedcall"(ptr @objc_retainAutoreleasedReturnValue) ]`
  compiled for `x86_64-pc-windows-msvc`. Inspect the post-expand-pseudo
  MIR's RegMask on the inserted `CALL64pcrel32` to the runtime — it will
  reflect the SysV preserve set, omitting RSI/RDI/R12-R15/XMM6-XMM15.
