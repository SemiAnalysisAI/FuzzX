# w375: X86InstrInfo::foldMemoryOperandImpl (LoadMI overload) mutates `MI` (TEST*rr -> CMP*ri/r,0) before subreg check; on failure MI is left mutated

## Component
`llvm/lib/Target/X86/X86InstrInfo.cpp` - `X86InstrInfo::foldMemoryOperandImpl(MF, MI, Ops, LoadMI, CopyMI, ...)` (overload that folds a load instruction into `MI`).

## Where
- `llvm/lib/Target/X86/X86InstrInfo.cpp:8304-8331`

```cpp
8304  if (Ops.size() == 2 && Ops[0] == 0 && Ops[1] == 1) {
8305    unsigned NewOpc = 0;
8306    switch (MI.getOpcode()) {
8307    default:
8308      return nullptr;
8309    case X86::TEST8rr:  NewOpc = X86::CMP8ri;    break;
...
8319    case X86::TEST64rr: NewOpc = X86::CMP64ri32; break;
8321    }
8322    // Change to CMPXXri r, 0 first.
8323    MI.setDesc(get(NewOpc));
8324    MI.getOperand(1).ChangeToImmediate(0);
8325  } else if (Ops.size() != 1)
8326    return nullptr;
8327
8328  // Make sure the subregisters match.
8329  // Otherwise we risk changing the size of the load.
8330  if (LoadMI.getOperand(0).getSubReg() != MI.getOperand(Ops[0]).getSubReg())
8331    return nullptr;
```

## Bug
For `TEST*rr` (op 0 == op 1 of the same vreg), the code:
1. Calls `MI.setDesc(get(CMP*ri))` (line 8323)
2. Mutates `MI.getOperand(1)` from a register to immediate 0 (line 8324)

It then runs further checks - the subreg check at 8330 - that can return `nullptr` (e.g., when `LoadMI`'s defined operand has a different subreg than `MI`'s operand 0). On that nullptr return, `MI` has been *permanently mutated in place* from `TEST*rr %r, %r` into `CMP*ri %r, 0`.

The caller `TargetInstrInfo::foldMemoryOperand` (`llvm/lib/CodeGen/TargetInstrInfo.cpp:847-851`) returns nullptr to its caller, but `MI` is left mutated. While `TEST*rr %r, %r` and `CMP*ri %r, 0` happen to set EFLAGS identically when the operand is itself, the *MachineInstr identity* is silently changed. Subsequent passes that inspect `MI.getOpcode()` (expecting TEST) or that try to re-fold the original TEST instruction will not see TEST anymore.

Similar mutate-before-fail pattern exists in the FrameIndex overload (`X86InstrInfo.cpp:7740-7745`), but `Impl()` there has its own internal guards (see commute-undo at 7660). In the LoadMI overload, the subreg mismatch at line 8330 *follows* the mutation with no rollback path.

Concretely, an additional load-fold attempt (e.g., from another spill site or another caller of `foldMemoryOperand` for the same MI later) will look up `CMP32ri`/`CMP64ri32` in the fold tables and produce different code than would have been produced by another TEST fold attempt (CMP fold uses different load addressing constraints than TEST).

## Repro hypothesis
Triggering requires:
- `Ops = {0, 1}` and `MI.getOpcode() == TEST*rr`
- `LoadMI.getOperand(0).getSubReg() != MI.getOperand(0).getSubReg()`

This is rare in straightforward IR because `TEST*rr` is typically generated with both operands the same plain vreg (no subreg). Reproducing requires an environment where a TEST's vreg operand has a subreg (e.g., after subreg coalescing) while the spilled load defines a non-subreg full reg, or vice versa.

A speculative .ll exercising TEST fold (does *not* trigger the bad path, but shows the entry-point exercised at -O2):

```ll
target triple = "x86_64-unknown-linux-gnu"

define i32 @t64(i64* %p) {
  %v = load i64, i64* %p, align 1
  %z = icmp eq i64 %v, 0
  %r = zext i1 %z to i32
  ret i32 %r
}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu` folds via `TEST64rr` -> `CMP64rm` path (visible in the asm as `cmpq $0, (%rdi)`).

## Severity / observability
- Low observable severity because TEST*rr %r,%r and CMP*ri %r,0 set EFLAGS identically.
- Real defect: a documented "do X first" mutation has no rollback when subsequent guards fail, which violates the typical "fold returns nullptr ==> instruction unchanged" caller contract elsewhere in `TargetInstrInfo::foldMemoryOperand` (see line 770-792 - the FrameIndex caller assumes original `MI.memoperands()` and `MI`'s ops are intact for fallback paths at 794-816 that compute `isCopyInstr(MI)`).

## Fix sketch
Move the subreg compatibility check (line 8330) *before* the TEST->CMP mutation at lines 8322-8324; or undo the mutation if any subsequent check returns nullptr.

## Confidence
Medium. Real state-corruption pattern; observable codegen impact requires a downstream path that re-folds or re-inspects the original TEST.
