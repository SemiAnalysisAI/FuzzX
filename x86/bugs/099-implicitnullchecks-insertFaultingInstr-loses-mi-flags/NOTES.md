# ImplicitNullChecks::insertFaultingInstr drops the original load's MI flags and def operand flags

File: llvm/lib/CodeGen/ImplicitNullChecks.cpp, function `insertFaultingInstr`
(lines 704-746).

## Pattern

When folding a regular load into a FAULTING_OP, the function constructs the new
instruction as:

```cpp
auto MIB = BuildMI(MBB, DL, TII->get(TargetOpcode::FAULTING_OP), DefReg)
               .addImm(FK)
               .addMBB(HandlerMBB)
               .addImm(MI->getOpcode());

for (auto &MO : MI->uses()) {
  if (MO.isReg()) {
    MachineOperand NewMO = MO;
    if (MO.isUse())  NewMO.setIsKill(false);
    else             NewMO.setIsDead(false);   // implicit def
    MIB.add(NewMO);
  } else {
    MIB.add(MO);
  }
}

MIB.setMemRefs(MI->memoperands());
```

Things this DROPS:

1. `DebugLoc DL;` (line 706) — empty DebugLoc. The original `MI->getDebugLoc()`
   is discarded. The faulting load loses its source-location attribution, which
   matters for null-deref crash reports that consult debug info.

2. `MI->getFlags()` — `MachineInstr::MIFlag` bits like `Unpredictable`,
   `NoMerge`, `FmReassoc` etc., are not copied. For loads the most relevant
   flag is `MIFlag::Unpredictable` (LangRef !unpredictable metadata); after
   folding, downstream passes that consult `isUnpredictable()` on the
   FAULTING_OP will see false.

3. The explicit-def operand at MI operand 0 carries metadata that is lost:
   - `isRenamable()` from the original def — after wrapping, the FAULTING_OP's
     def is constructed by `BuildMI(..., DefReg)` which produces a plain
     `RegState::Define` operand with no renamable bit. Subsequent
     post-RA register-renaming passes (e.g. `MachineCopyPropagation` itself
     keys on `isRenamable()`) will refuse to operate on this def even though
     the original load's def was renamable.
   - `isEarlyClobber()` (rare for a single-def load but technically possible
     for fused load+RMW pseudos) — silently dropped.
   - subreg index on the def is silently dropped: `DefReg` is the bare
     `getReg()`, no `getSubReg()`. If the load originally wrote a subreg, the
     FAULTING_OP now claims to write the full register.

4. `MI->isReturn()`, `MI->isCall()`, etc. flags are not in scope here because
   `canHandle` rejects calls; OK.

The MMOs themselves are correctly propagated via `setMemRefs` so MMO flags
(volatile/atomic/dereferenceable/nontemporal) survive — that part is fine.

## Why this matters

- (3) is the most concrete correctness concern: a subreg-write load
  (e.g. `MOV8rm` writing AL, with no implicit super-reg def) folded into
  FAULTING_OP that asserts a write to the full subreg-less register triggers
  a live-range expansion that the verifier or RegisterCoalescer may
  miscompile downstream. ImplicitNullChecks runs after RA
  (`getRequiredProperties = setNoVRegs`), so operands are physregs by the
  time we get here, but MachineInstr-level subreg writes are still meaningful
  for sub-byte registers and for x86's GR8 (AH vs AL) class.

- (2) `MIFlag::Unpredictable` semantically forbids reorder/merge across the
  load. Losing it under a FAULTING_OP wrapper means subsequent passes that
  honor `isUnpredictable()` won't see the constraint and may, e.g., merge
  this fault-trapping load with another load.

- (1) is cosmetic but interacts with !dbg-driven crash diagnostics in
  JIT/runtime contexts (which is exactly the user of FaultMaps).

## Fix sketch

```cpp
auto MIB = BuildMI(MBB, MI->getDebugLoc(),
                   TII->get(TargetOpcode::FAULTING_OP), DefReg);
MIB.setMIFlags(MI->getFlags());
// then preserve the original def's subreg/renamable/earlyclobber bits onto
// MIB.getInstr()->getOperand(0).
```

## Confidence

Medium. (3) is the most likely real miscompile root in practice for the
GR8 AH/AL family on x86-64 (subreg writes that get promoted to whole-register
writes). The other concerns are correctness-adjacent (debug, scheduler
predicates).

The structural bug — DL is uninitialized and `MI->getFlags()` is dropped — is
unambiguous from the source.

## Reproduction sketch (not yet executed)

The reproduction requires `!make.implicit` metadata on a branch terminator, so
needs IR with explicit metadata:

```llvm
define i8 @f(ptr %p) {
entry:
  %nn = icmp eq ptr %p, null
  br i1 %nn, label %null, label %notnull, !make.implicit !0
notnull:
  %v = load i8, ptr %p, !unpredictable !1
  ret i8 %v
null:
  call void @throw()
  unreachable
}
declare void @throw()
!0 = !{}
!1 = !{}
```

Build with `llc -mtriple=x86_64-linux-gnu -enable-implicit-null-checks`.
Inspect the resulting `FAULTING_OP` MIR for the preserved `!unpredictable`
attribution (MIFlag::Unpredictable). Expected: preserved; observed (per
source reading): absent.
