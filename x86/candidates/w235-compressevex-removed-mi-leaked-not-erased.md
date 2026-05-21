# w235: X86CompressEVEX ADD_ND-to-LEA compression removeFromParent without erase

## Summary

When `X86CompressEVEX::CompressEVEXImpl` rewrites a non-redundant `ADD32ri_ND`/`ADD64ri32_ND`/`ADD32rr_ND`/`ADD64rr_ND` into a `LEA`, it calls `MI.removeFromParent()` and returns `true`. The MI is unlinked from its basic block but is NEVER added to the `ToErase` vector and never `eraseFromParent()`ed. Consequences:

1. The MachineInstr object stays in `MachineFunction`'s allocator until the MF is destroyed (latent storage leak per function-call site that triggers this path).
2. All operands of the orphaned MI still hold references into `MachineRegisterInfo`'s use-def chains. After `removeFromParent`, those chain entries are stale but not removed (whereas `eraseFromParent` properly removes them via `removeRegOperandsFromUseLists`). The result is **stale entries in `MRI.use_list(VReg)` and `MRI.def_list(VReg)`** for any virtual register the orphaned MI referenced. Subsequent passes that iterate `MRI->use_nodbg_instructions(...)` will visit the orphan instruction even though its parent is `nullptr`.
3. If a later pass calls `MI->getParent()` on the orphan, it crashes (null deref) or returns null and produces wrong results in downstream analyses (e.g., MachineDominator queries).

## Source location

`llvm/lib/Target/X86/X86CompressEVEX.cpp` lines 413-440:

```c++
413   } else if (Opc == X86::ADD32ri_ND || Opc == X86::ADD64ri32_ND ||
414              Opc == X86::ADD32rr_ND || Opc == X86::ADD64rr_ND) {
...
427       MachineInstrBuilder MIB = BuildMI(MBB, MI, MI.getDebugLoc(), NewDesc, Dst)
428                                     .addReg(Src1)
429                                     .addImm(1);
...
437       MI.removeFromParent();          // <<< unlinks, but operands remain in use-lists
438       return true;                    // <<< caller never adds to ToErase
439     }
```

Caller `runOnMF` at lines 500-510 iterates `make_early_inc_range(MBB)` and pushes "to-erase" MIs via the (now-unused) `ToErase` vector:

```c++
500   for (MachineBasicBlock &MBB : MF) {
501     SmallVector<MachineInstr *, 4> ToErase;
502     for (MachineInstr &MI : llvm::make_early_inc_range(MBB)) {
503       Changed |= CompressEVEXImpl(MI, MBB, ST, ToErase);   // <<< caller's only sink
504     }
505     for (MachineInstr *MI : ToErase) {
506       MI->eraseFromParent();
507     }
508   }
```

The two other paths in `CompressEVEXImpl` that wish to remove MIs (`tryCompressVPMOVPattern` at line 316 and the redundant-NDD path that does `setDesc` only) DO use `ToErase`. The ADD_ND-to-LEA path is the lone outlier that calls `removeFromParent`.

## Reproducer scaffold

```ll
target triple = "x86_64-unknown-linux-gnu"

define i64 @test(i64 %a) "target-features"="+ndd" {
  %r = add i64 %a, 13
  ret i64 %r
}
```

`llc -mtriple=x86_64-unknown-linux-gnu -O2 -mattr=+ndd /tmp/x86bugs/test_compress_ndd.ll -o -`. Pre-CompressEVEX MIR contains `ADD64ri32_ND $rdi, 13`; post-CompressEVEX, this is rewritten to a `LEA64r` and the original ADD MI is orphaned. The ADD's operand $rdi (and EFLAGS implicit def) are still recorded in MRI's use-def chains.

## Impact

In current `-O2` pipelines, no subsequent MIR pass iterates orphaned MIs after CompressEVEX (the pass runs near end of pipeline). So today the bug does not manifest as wrong-asm. But:

- ANY future pass added after CompressEVEX that traverses MRI use-def chains would visit the orphan and crash on `MI->getParent()`.
- Memory is leaked per ADD-to-LEA conversion.
- The pattern is contrary to the contract documented in `MachineInstr.h`: "After `removeFromParent`, you typically want `deleteMachineInstr` (or `eraseFromParent`)."

## Fix

```diff
       MIB.addReg(0);
-      MI.removeFromParent();
+      ToErase.push_back(&MI);
       return true;
     }
```

at line 437. One-line fix.
