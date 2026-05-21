# w376: X86 `fuseInst` drops every MachineInstr MIFlag except `NoFPExcept` on memory-operand fold

## Component
`llvm/lib/Target/X86/X86InstrInfo.cpp` - static `fuseInst()` (and `fuseTwoAddrInst()` which has the same gap).

## Where
- `llvm/lib/Target/X86/X86InstrInfo.cpp:7317-7347` (`fuseInst`)
- `llvm/lib/Target/X86/X86InstrInfo.cpp:7288-7315` (`fuseTwoAddrInst`)

```cpp
7317  static MachineInstr *fuseInst(MachineFunction &MF, unsigned Opcode,
...
7323    MachineInstr *NewMI =
7324        MF.CreateMachineInstr(TII.get(Opcode), MI.getDebugLoc(), true);
...
7327    for (unsigned i = 0, e = MI.getNumOperands(); i != e; ++i) {
...   (copies operands)
7335    }
7337    updateOperandRegConstraints(MF, *NewMI, TII);
7338
7339    // Copy the NoFPExcept flag from the instruction we're fusing.
7340    if (MI.getFlag(MachineInstr::MIFlag::NoFPExcept))
7341      NewMI->setFlag(MachineInstr::MIFlag::NoFPExcept);
7342
7343    MachineBasicBlock *MBB = InsertPt->getParent();
7344    MBB->insert(InsertPt, NewMI);
7345
7346    return MIB;
7347  }
```

`fuseTwoAddrInst` (lines 7288-7315) has *no* flag transfer at all - not even `NoFPExcept`.

## Bug
The selection-DAG `InstrEmitter::AddOperand`/`AddFlags` (`llvm/lib/CodeGen/SelectionDAG/InstrEmitter.cpp:1079-1126`) sets the following MIFlags from corresponding SDNodeFlags on every emitted MachineInstr at SDISel time:

- `Unpredictable` (1079)
- `FmNsz` (1087), `FmArcp` (1090), `FmNoNans` (1093), `FmNoInfs` (1096), `FmContract` (1099), `FmAfn` (1102), `FmReassoc` (1105)
- `NoUWrap` (1108), `NoSWrap` (1111), `IsExact` (1114)
- `NoFPExcept` (1117), `Disjoint` (1120), `SameSign` (1123)
- `NoConvergent` (1126)

When a `MachineInstr` is folded with a memory operand by `fuseInst`/`fuseTwoAddrInst`, *only* `NoFPExcept` is preserved (line 7340-7341 of X86InstrInfo.cpp). Every other flag above is silently dropped on the new fused instruction.

This means - after memory-operand folding (a regalloc-time operation) - the following information is lost:

1. **Branch flags**: `Unpredictable` (used by branch folding/layout heuristics) - dropped if a fused instruction was a branch (rare for X86 but the gap is general).
2. **Fast-math flags** (`FmNsz`, `FmReassoc`, `FmContract`, `FmNoNans`, `FmNoInfs`, `FmAfn`, `FmArcp`) - used by `MachineCombiner` (`llvm/lib/CodeGen/MachineCombiner.cpp`) and X86's reassociation paths (see `X86InstrInfo::getMachineCombinerPatterns` for FMA reassociation) to decide whether reassociation/FMA contraction is legal. Dropping these on a fused FMA-like or FADD/FMUL instruction can suppress legitimate combining post-fold (codegen quality / missed optimization), or - more concerning - re-enable reassociation that the IR explicitly forbade if other passes use these flags to *guard* unsafe transforms.
3. **Integer wrap flags** (`NoUWrap`, `NoSWrap`, `IsExact`) - relevant for ADD/SUB/LEA folding heuristics and any post-regalloc pass that might inspect these.
4. **`Disjoint`/`SameSign`** - integer hint flags used by recent backend simplifications.

## Where in X86 it matters
`fuseInst`/`fuseTwoAddrInst` is the construction path used for *all* X86 memory-operand folds (see `X86InstrInfo.cpp:7609-7610`):

```cpp
NewMI = IsTwoAddr ? fuseTwoAddrInst(MF, Opcode, MOs, InsertPt, MI, *this)
                  : fuseInst(MF, Opcode, OpNum, MOs, InsertPt, MI, *this);
```

It is reached by both spill/reload memory fold (FrameIndex overload) and load-fold (LoadMI overload). After register allocation, `MachineCombiner` runs in post-RA mode (see `X86PassConfig::addMachineSSAOptimization`) and inspects MIFlags on FMA / FADD / FMUL instructions to drive reassociation.

## Repro hypothesis
Construct a function whose `fadd` / `fmul` / `fma` has IR-level `fast` (or `reassoc contract nsz`) flag, is selected, then has one input promoted to a fold via load-fold. Verify that the resulting fused operation no longer carries any `Fm*` MIFlag. A `-debug-only=isel` or MIR dump after `*-machine-instrs` will show the lost flags.

A speculative .ll exercising the fold path:

```ll
target triple = "x86_64-unknown-linux-gnu"

define float @f(ptr %p, float %y) {
  %x = load float, ptr %p
  %r = fadd reassoc contract nsz float %x, %y
  ret float %r
}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu` folds the load into the addss (visible as `addss (%rdi), %xmm0`). MIR dumped after the fold will show the new VADDSSrm/ADDSSrm with no `reassoc`/`contract`/`nsz` MIFlags (only `NoFPExcept` if the original carried it).

## Severity
Codegen quality (missed `MachineCombiner` fma/reassoc opportunities post-fold). In principle could enable an unsafe transform if a future pass uses absence of `nsz`/`contract` as a permissive marker (i.e., the inverse of what the IR encoded). Today the practical impact is mostly missed optimization, but it is also a latent correctness footgun for any new pass that consults MIFlags after regalloc memory-fold.

## Fix sketch
Replace the single-flag transfer at 7340-7341 with the same set used by SDISel's `InstrEmitter::AddFlags`, or simply `NewMI->setFlags(MI.getFlags())` (clearing flags that don't apply to the *new* opcode, e.g., FrameSetup/FrameDestroy).

## Confidence
High that this is a real omission; medium-low for observable defect on default x86 -O2 today (codegen-quality / missed-opt).
