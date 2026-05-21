# 018 — `X86ExpandPseudo::expandCALL_RVMARKER` uses SysV regmask on Win64

Component: X86ExpandPseudo

## Source

`llvm/lib/Target/X86/X86ExpandPseudo.cpp:241-261`

The expansion of `CALL_RVMARKER` for `clang.arc.attachedcall` bundles
contains two paths gated on `isOSWindows()`:

```cpp
auto TargetReg = STI->getTargetTriple().isOSWindows() ? X86::RCX : X86::RDI;
// ... emit "movq %rax, TargetReg" marker ...

const uint32_t *RegMask =
    TRI->getCallPreservedMask(*MBB.getParent(), CallingConv::C);   // <-- BUG
```

The marker register (RDI on SysV / RCX on Win64) is selected correctly per
ABI, but the preserved-register mask is **always** `CallingConv::C` (SysV).
Win64 preserves a different and larger set (RSI, RDI, R12-R15, XMM6-XMM15).
On Win64 the constructed call instruction therefore claims to clobber
registers that the Win64 ABI actually preserves.

Today's symptom is over-conservative codegen — the caller spills more
registers than necessary across the marker call. But the comment block
above clearly shows the author was aware of the Win64 distinction; the
mask is inconsistent with the rest of the pass. If any downstream pass
relies on the regmask to compute live-out (e.g., reg-alloc liveness across
the call), bigger problems can surface.

## Fix

```cpp
CallingConv::ID RtCC = STI->getTargetTriple().isOSWindows()
                       ? CallingConv::Win64 : CallingConv::C;
const uint32_t *RegMask = TRI->getCallPreservedMask(*MBB.getParent(), RtCC);
```

## Repro

```ll
; llc -mtriple=x86_64-pc-windows-msvc reduce.ll
target triple = "x86_64-pc-windows-msvc"
declare ptr @objc_retainAutoreleasedReturnValue(ptr)
declare ptr @foo()
define ptr @bar() {
  %r = call ptr @foo() [ "clang.arc.attachedcall"(ptr @objc_retainAutoreleasedReturnValue) ]
  ret ptr %r
}
```

Inspect MIR after expand-pseudo and look at the regmask on the inserted
runtime call — it will be SysV-shaped (missing RSI/RDI/R12-R15/XMM6-XMM15).

Source-confirmed.
