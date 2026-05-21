# IBT: WinEH catch/cleanup funclet entry blocks lack ENDBR

File: llvm/lib/Target/X86/X86IndirectBranchTracking.cpp:98-112, 156-181

```
// needsPrologueENDBR:
default:
  return (F.hasAddressTaken() || !F.hasLocalLinkage());

// EH handling:
if (TM->Options.ExceptionModel == ExceptionHandling::SjLj) { ... }
else if (MBB.isEHPad()){
  for (... I != MBB.end(); ++I) {
    if (!I->isEHLabel())
      continue;
    Changed |= addENDBR(MBB, std::next(I));
    break;
  }
}
```

## Reasoning

WinEH (MSVC personality, x86_64-pc-windows-msvc) lowers each catchpad
and cleanuppad into a SEPARATE `MachineFunction` (funclet) emitted as
its own symbol (e.g. `"?catch$2@?0?f@4HA"`). The OS exception dispatcher
transfers control to that funclet address INDIRECTLY through the SEH
unwind/handler data — i.e. exactly the pattern that CET IBT is
designed to gate with `endbr64`.

The IBT pass handles EH in two places:

1. `needsPrologueENDBR(MF)` — gates ENDBR at the function entry on
   `hasAddressTaken() || !hasLocalLinkage()`. A WinEH funclet's
   `Function` is the same parent `Function`; the funclet does not
   itself have its address taken via an LLVM IR `blockaddress`,
   so `hasAddressTaken()` is false. Even if `!hasLocalLinkage()`
   triggers ENDBR for the parent function, that ENDBR lands on
   the PARENT's entry block — not on the funclet's entry MBB
   inside that same MF.

2. The per-MBB `else if (MBB.isEHPad())` branch scans for an
   `EHLabel` and inserts ENDBR after it. WinEH funclet entry MBBs
   do not begin with an `EH_LABEL` (the labels `Ltmp0/Ltmp1`
   bracket the invoke in the PARENT block, not the funclet entry).
   The loop therefore walks the whole MBB without finding an
   `EHLabel`, breaks out implicitly, and inserts NO ENDBR.

The result, demonstrated below, is a `"?catch$..."` funclet that the
OS dispatcher jumps to without an `endbr64` at its first byte, so the
process will `#CP` fault on Tiger Lake / Sapphire Rapids when CET-IBT
is enforced — silently disabling exception handling under IBT, which
is the opposite of the user's intent when they enabled
`cf-protection-branch`.

## IR repro sketch

```
; llc -mtriple=x86_64-pc-windows-msvc reduce.ll
target triple = "x86_64-pc-windows-msvc"
declare i32 @__CxxFrameHandler3(...)
declare void @throws()

define void @f() personality ptr @__CxxFrameHandler3 {
entry:
  invoke void @throws() to label %cont unwind label %cd
cont:
  ret void
cd:
  %cs = catchswitch within none [label %c] unwind to caller
c:
  %cp = catchpad within %cs [ptr null, i32 64, ptr null]
  catchret from %cp to label %cont
}
!llvm.module.flags = !{!0}
!0 = !{i32 8, !"cf-protection-branch", i32 1}
```

Observed (verified locally) — `"?catch$2@?0?f@4HA":` funclet entry has
NO `endbr64`; only the catchret target `.LBB0_1` (block-address-taken
MBB inside the parent funclet) gets one.

## Expected wrong outcome

On a CET-enforcing CPU/OS configuration, any exception thrown by
`@throws()` causes a `#CP` (control protection) fault when the OS
dispatcher jumps into `"?catch$..."`, even though the user requested
`cf-protection-branch` precisely to make IBT safe. The mitigation must
emit `endbr64` at the first instruction of every funclet MBB that is
the entry of a separate MachineFunction.
