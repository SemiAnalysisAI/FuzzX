# X86CmovConversion: unfoldMemoryOperand result inserted into FalseMBB without checking EFLAGS clobbers

File: llvm/lib/Target/X86/X86CmovConversion.cpp:728-826

## Reasoning

When a CMOV has a memory operand (under `ForceMemOperand`), the pass unfolds the load using `TII->unfoldMemoryOperand`, producing two new MIs:

- `NewCMOV` — the register-only CMOV, inserted into MBB before the original CMOV (line 793)
- `NewMI` — the load instructions, inserted into `FalseMBB` (line 804)

The load is inserted into FalseMBB which sits between the original MBB and SinkMBB. The pass does check `checkEFLAGSLive(LastCMOV)` to decide if FalseMBB/SinkMBB need `addLiveIn(X86::EFLAGS)`. But the inserted load (e.g. `MOV64rm`) does not clobber EFLAGS, so that part is fine.

The bug is more subtle: when the unfolded load instruction has its *own* implicit defs (for some opcodes, the unfolded base instruction might define EFLAGS, e.g., a folded `ADD64rm` cmov unfold yields `ADD64rr` which DOES clobber EFLAGS). The code at line 781 invokes `unfoldMemoryOperand` with `UnfoldLoad=true, UnfoldStore=false`. The returned `NewMIs` list contains the load MI(s); `NewMIs.pop_back_val()` retrieves the *last* one as `NewCMOV` (line 789), asserting it is a CMOV. The remaining `NewMIs` are sunk into FalseMBB at FalseInsertionPoint (line 802-804). For a CMOV opcode `CMOV32rm`/`CMOV64rm` the unfold yields a plain `MOV32rm/MOV64rm` load + a `CMOV32rr/CMOV64rr` register-cmov. These loads don't define EFLAGS — OK.

But the code under `ForceMemOperand` allows a *group* of CMOVs to share the same FalseMBB. After unfolding the first cmov, `FalseInsertionPoint` is set to `FalseMBB->begin()` (line 720) and *not advanced*. Each subsequent unfolded load is inserted at the same `FalseMBB->begin()`, meaning the order of loads in FalseMBB is reversed relative to the original CMOV order. If the loads reference each other via the FalseBBRegRewriteTable (line 727: walks through earlier cmovs' dests to find the load address sources), the rewriting picks `It->second` for each operand. But because FalseInsertionPoint never advances, a later inserted load that *uses* a vreg defined by an *earlier* inserted load would be placed *before* the earlier load in FalseMBB — leading to use-before-def in MachineSSA.

The condition for this: two chained CMOVrm where the load address of CMOV2 depends on the destination of CMOV1. Since the rewrite table maps a CMOV's dest reg → false-side reg, when CMOV2's load address operand was the dest of CMOV1, line 806-820 rewrites the operand to point at CMOV1's false-side load TmpReg. Both loads land in FalseMBB at position `FalseInsertionPoint` (still `FalseMBB->begin()` because we never bumped it). Insertion at begin() pushes the new load to the front, so the second load (which uses the first load's TmpReg) is placed before the first load. Verifier failure.

## MIR reproducer sketch

```
bb.0:
  liveins: $rdi, $rsi
  CMP64rr $rdi, $rsi, implicit-def $eflags
  %2:gr64 = CMOV64rm $rdi, $rdi, 1, $noreg, 0, $noreg, 4, implicit $eflags
  %3:gr64 = CMOV64rm $rsi, %2,  1, $noreg, 0, $noreg, 4, implicit $eflags
  ; ^^ second cmov loads from address derived from %2, the dest of the first cmov
  $rax = COPY %3
  RET 0, $rax
```

Force unfolding via `-x86-cmov-converter-force-mem-operand=true` (default true) inside a loop.

## Expected wrong outcome

After conversion, FalseMBB has the two MOV64rm loads in *reverse* order: the load that uses the first load's result appears first, producing a use-before-def. `llc -verify-machineinstrs -O2` will assert: "Virtual register defined after its use" or similar SSA violation. In release builds this can produce wrong code (the second load reads an undefined register).
