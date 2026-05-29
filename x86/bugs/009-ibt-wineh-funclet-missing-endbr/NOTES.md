# 009 — IBT misses WinEH catch/cleanup funclet entries (CET-IBT mitigation gap)

Component: X86IndirectBranchTracking

## Source

`llvm/lib/Target/X86/X86IndirectBranchTracking.cpp` — two relevant places:

1. `needsPrologueENDBR` (around line 98) gates the ENDBR at function entry
   on `F.hasAddressTaken() || !F.hasLocalLinkage()`. This places ENDBR at the
   parent function's entry — but a WinEH catch / cleanup funclet is emitted
   as a separate `MachineFunction` whose entry is a different MBB inside the
   same parent IR `Function`.
2. The per-MBB scan around lines 156-181:

   ```cpp
   } else if (MBB.isEHPad()) {
     for (...; I != MBB.end(); ++I) {
       if (!I->isEHLabel())
         continue;
       Changed |= addENDBR(MBB, std::next(I));
       break;
     }
   }
   ```

   This looks for an `EH_LABEL` to place the ENDBR after — but WinEH funclet
   entry MBBs do not begin with `EH_LABEL`; the `Ltmp*` labels bracket the
   invoke in the *parent* MBB. The scan completes without finding one and
   inserts nothing.

## Why this matters

With CET-IBT enabled (`!"cf-protection-branch"` module flag), every
indirect branch / call target must start with `endbr64`. WinEH on
x86_64-pc-windows-msvc lowers each catch / cleanup pad to a separate
symbol (e.g., `"?catch$2@?0?f@4HA"`) whose address is reachable only
indirectly through the SEH unwind tables — exactly the threat model
IBT is designed for.

## Demonstration

`repro.ll`: a trivial `try / catch` with `cf-protection-branch=1`.

`./cmd.sh` shows (relevant excerpt):

```
f:
.seh_proc f
.seh_handler __CxxFrameHandler3, @unwind, @except
        endbr64                 ; parent entry — present
        ...
.LBB0_1:                        ; block-address-taken catchret target — present
        endbr64
        ...
"?catch$2@?0?f@4HA":
.seh_proc "?catch$2@?0?f@4HA"
.seh_handler __CxxFrameHandler3, @unwind, @except
.LBB0_2:                        ; %c (catch funclet entry — NO endbr64!)
        .seh_pushreg %rbp
        ...
```

The catch funclet starts with `.seh_pushreg %rbp`, not `endbr64`. On a CET
IBT-enforcing CPU/OS, the first instruction the OS dispatcher jumps to in
the funclet would not be a valid IBT landing pad and the process would
`#CP` fault — turning every exception into a crash, the opposite of what
the user asked for when enabling `cf-protection-branch`.

## Fix sketch

In `X86IndirectBranchTracking::runOnMachineFunction`, additionally insert
ENDBR at the entry of any MBB that is the **first MBB of a funclet** (i.e.,
`MBB.isEHFuncletEntry()`), independently of the EH_LABEL search.

## Severity

CET-IBT mitigation gap. Affects any WinEH C++ code compiled with
`-fcf-protection=branch` on a CET-enforcing kernel — the program will
fault as soon as it throws, despite asking the compiler for IBT.

## Verdict: WONTFIX (no enforcing platform)

The "CET-enforcing kernel" in the severity note does not exist for this
code. Funclets are produced only by WinEH, and Windows does not implement
IBT: it uses Control Flow Guard / XFG for the forward edge and adopts only
CET's shadow stack (backward edge). So on every shipping Windows the
emitted `endbr64` are inert NOPs and the `#CP` fault cannot occur — there
is no OS that both runs funclet-based EH and enforces IBT.

clang still accepts `-fcf-protection=branch` for a Windows triple with no
diagnostic, and LLVM does emit `endbr64` on the parent entry / EH-pad
blocks under that flag, so the missing funclet `endbr64` is a genuine
codegen *inconsistency* — but purely theoretical hardening, not an
observable bug. The more defensible fix is to reject/warn on
`-fcf-protection=branch` for Windows targets in the driver rather than to
keep extending dead IBT codegen. PR #200333 dropped.
