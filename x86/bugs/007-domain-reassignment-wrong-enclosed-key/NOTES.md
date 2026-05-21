# 007 — X86DomainReassignment `EnclosedEdges[Reg]` uses outer seed `Reg`, not `CurReg`

Component: X86DomainReassignment

## Source

`llvm/lib/Target/X86/X86DomainReassignment.cpp:546-557`

```cpp
void X86DomainReassignmentImpl::buildClosure(Closure &C, Register Reg) {
  SmallVector<Register, 4> Worklist;
  RegDomain Domain = NoDomain;
  visitRegister(C, Reg, Domain, Worklist);
  while (!Worklist.empty()) {
    Register CurReg = Worklist.pop_back_val();

    // Register already in this closure.
    if (!C.insertEdge(CurReg))
      continue;
    EnclosedEdges[Reg] = C.getID();   // <-- BUG: should be EnclosedEdges[CurReg]
    ...
```

The local `CurReg` is the register being added to the closure on this
iteration; `Reg` is the original seed passed into `buildClosure`. The
assignment overwrites the same key (`Reg`) on every iteration, so the
`EnclosedEdges` map records only the seed of each closure, never any of the
other members.

`EnclosedEdges` is then consulted in two places:

1. `visitRegister` (line 428): used to detect when a register already belongs
   to *another* closure and call `C.setAllIllegal()`. With the bug, that
   detection only fires for the seed — all other intra-closure members look
   "not yet enclosed."
2. The outer driver loop in `runOnMachineFunction` (line 809): skips vregs
   that are already enclosed before launching a fresh `buildClosure`. With
   the bug, the driver re-walks every non-seed vreg of every closure as a
   fresh seed, building duplicate / overlapping closures.

Today the bug is mostly masked by the parallel `EnclosedInstrs` map
(`encloseInstr` at line 452-462), which detects cross-closure conflicts at
the **instruction** level and calls `setAllIllegal()`. So the practical
effect is wasted closure-build work (each member walked O(closure-size)
times) and unused/illegal closures littering the worklist. There are
constructions of the IR where a register can belong to a closure without
its def-instr being enclosed (e.g., when the def is a `COPY` from a
non-virtual register) — in those, the EnclosedInstrs failsafe doesn't fire
and the second closure can call `reassign()` on a vreg whose class has
already been replaced by `VK*`, causing a verifier failure (or, in release,
register-class mismatch on the inserted KMOV*).

## Demonstration

This is a source-level off-by-name. The fix is trivial:

```diff
-    EnclosedEdges[Reg] = C.getID();
+    EnclosedEdges[CurReg] = C.getID();
```

(See `repro-discussion.md` for the IR pattern that exercises the
duplicate-closure path, but I have not yet driven it to a verifier crash.)

## Why this counts

A one-character typo in a transform that mutates SSA register classes is
exactly the kind of latent bug that gets activated by an unrelated change
elsewhere (e.g., a new converter for a new instruction that doesn't enclose
its DefMI). Worth fixing even when the failsafe currently saves us.
