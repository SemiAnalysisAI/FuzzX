# w230: X86FixupInstTuning ProcessShiftLeftToAdd returns false after modifying MI

## Summary

`X86FixupInstTuning::ProcessShiftLeftToAdd` (used for transforming `PSLL{W,D,Q} reg, 1` into `PADD{W,D,Q} reg, reg`) mutates the MachineInstr in place but always **returns false**. This causes:

1. `NumInstChanges` stat is never incremented when this transformation fires (silent fail of the stat).
2. The `X86FixupInstTuning` pass returns `Changed = false` when it actually mutated MIs, so the pass manager's `PreservedAnalyses` calculation incorrectly preserves CFGAnalyses / other passes that depend on instruction identity — even though the desc, opcode and operand list have all changed.
3. Other passes that re-check this pass's effect via the bool return cannot detect that `PSLL?ri imm=1` has been rewritten.

The intended behavior of every other Process* lambda in the file (`ProcessBLENDToMOV`, `ProcessBLENDWToBLENDD`, `ProcessUNPCK*`, `ProcessVPERM*`, `ProcessVPERMQToVINSERT128`, etc.) is to return `true` on a successful mutation. `ProcessShiftLeftToAdd` is the only outlier.

## Source location

File: `llvm/lib/Target/X86/X86FixupInstTuning.cpp`

```c++
294   // Is ADD(X,X) more efficient than SHL(X,1)?
295   auto ProcessShiftLeftToAdd = [&](unsigned AddOpc) -> bool {
296     if (MI.getOperand(NumOperands - 1).getImm() != 1)
297       return false;
298     if (!NewOpcPreferable(AddOpc, /*ReplaceInTie*/ true))
299       return false;
300     LLVM_DEBUG(dbgs() << "Replacing: " << MI);
301     {
302       MI.setDesc(TII->get(AddOpc));
303       MI.removeOperand(NumOperands - 1);
304       MI.addOperand(MI.getOperand(NumOperands - 2));
305     }
306     LLVM_DEBUG(dbgs() << "     With: " << MI);
307     return false;          // <<< BUG: should be `return true`
308   };
```

Compare with `ProcessBLENDToMOV` (line 279-292), `ProcessBLENDWToBLENDD` (line 258-277), `ProcessUNPCK` (line 210-220), all of which return `true` after the matching mutation block.

Callers at lines 634-669 are 18 switch cases for `PSLL{W,D,Q}ri` / `VPSLL{W,D,Q}{Y,Z128,Z256,Z}ri`. All inherit the wrong return.

## Reproducer (.ll)

```ll
target triple = "x86_64-unknown-linux-gnu"
define <8 x i16> @test(<8 x i16> %a) {
  %r = shl <8 x i16> %a, splat (i16 1)
  ret <8 x i16> %r
}
```

## Behavior

`llc -mtriple=x86_64-unknown-linux-gnu -O2 -mattr=+sse2`:

```asm
test:
        paddw   %xmm0, %xmm0          # transformation fired (paddw, not psllw)
        retq
```

The transformation is performed. Pre-pass MIR has `PSLLWri %xmm0, 1`; post-pass has `PADDWrr %xmm0, %xmm0`. But `runOnMachineFunction` returns `false` because `processInstruction` returned false (line 686-688):

```c++
if (processInstruction(MF, MBB, I)) {       // returns false even though MI was changed
  ++NumInstChanges;                          // never incremented
  Changed = true;                            // never set
}
```

Then in the new-PM `X86FixupInstTuningPass::run` (line 705-708), the pass returns `PreservedAnalyses::all()` because `Changed` was false — but in reality CFG-impacting analyses on the modified MI may now be stale.

## Impact

- **Wrong pass-manager preservation**. Other transforms run after X86FixupInstTuning may see stale info because the pass reports "no changes."
- **Silent stats**. `NumInstChanges` reports an undercount for any function where only `ProcessShiftLeftToAdd` mutations fire (no MIs other than PSLL?ri with imm=1).
- **Reaches into legalizer-output correctness**. Although the asm itself is the intended PADDWrr, any subsequent MI-pass that gates on "did FixupInstTuning run?" via the bool return will misbehave.

## Fix

```diff
-    return false;
+    return true;
   };
```

at line 307. One-character class of change.

## Affected opcodes

PSLLWri, PSLLDri, PSLLQri, VPSLLWri, VPSLLDri, VPSLLQri, VPSLLWYri, VPSLLDYri, VPSLLQYri, VPSLLWZ128ri, VPSLLDZ128ri, VPSLLQZ128ri, VPSLLWZ256ri, VPSLLDZ256ri, VPSLLQZ256ri, VPSLLWZri, VPSLLDZri, VPSLLQZri.
