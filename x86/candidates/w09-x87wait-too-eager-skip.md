## Candidate: X86InsertX87Wait skips WAIT before a non-synchronizing X87 successor

File: /home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Target/X86/X86InsertX87Wait.cpp:118-121

### Reasoning
```
MachineBasicBlock::iterator AfterMI = std::next(MI);
if (AfterMI != MBB.end() && X86::isX87Instruction(*AfterMI) &&
    !isX87NonWaitingControlInstruction(*AfterMI))
  continue;
```
The pass omits inserting `WAIT` after the current x87 instruction if the immediately
following instruction is any X87 instruction other than one of the FN* non-waiting
controls (FNINIT/FNSTSW/FNSTCW/FNCLEX). The intent is "the next x87 instruction will
itself synchronize exceptions, so the explicit WAIT is redundant".

This is unsound for two reasons:

1. The `isX87NonWaitingControlInstruction` set is incomplete. There are additional
   non-waiting x87 instructions that begin with `FN` (e.g. `FNSAVE`, `FNRSTOR`, the
   ENV variants `FNSTENV`/`FNLDENV` which exist in encoded forms even though the
   table at line 67-69 lists the waiting `FSAVEm`/`FRSTORm`/`FLDENVm`/`FSTENVm`). If
   the next instruction is an `FNSAVE`/`FNSTENV` form the WAIT is skipped, but those
   instructions do NOT raise pending FP exceptions, so the prior instruction's
   exception can be lost across the boundary.

2. Skipping the WAIT based on physical adjacency in `MachineBasicBlock::iterator`
   order is incorrect at the end of a block. `AfterMI = std::next(MI)` may point at
   `MBB.end()`, in which case the check rightly fails. But the *successor* block's
   first instruction could be another x87 op; the pass does not look across BBs.
   So at a fall-through edge the WAIT is correctly inserted, but the same x87 op
   sequenced through a CFG join (e.g. multiple predecessors all ending with an FP
   trap-eligible op + one successor whose first instruction is an x87 sync op) is
   over-conservative — not a miscompile, but the simple intra-block omission rule
   IS incorrect when the very next iterator slot is a meta-instruction (DBG_VALUE,
   CFI_INSTRUCTION) which is not skipped: `isX87Instruction` returns false for
   meta instrs, so the code falls through and inserts the WAIT correctly there.
   Conversely, the absence of meta-skipping means a debug-build's output can
   differ from a release-build's, since debug values can break the adjacency
   that triggers the skip.

### Repro sketch
Strict-fp function with `FCOM`+`FNSAVE` pair, optionally with intervening DBG_VALUE:
```
declare void @foo(double) strictfp
define void @bar() strictfp {
  ; ... arrange a strict FCOM where the next pseudo emitted is FNSAVE
}
```
Compare `-O0` (with debug intrinsics) vs `-O0 -g0` to see WAIT inserted in one but
not the other for the same logical sequence.

### Wrong outcome
Either (a) a lost FP exception across `FNSAVE`/`FNSTENV`, or (b) WAIT placement
that depends on the presence of debug info, leading to ABI-observable differences
between `-g` and non-`-g` strict-fp builds.
