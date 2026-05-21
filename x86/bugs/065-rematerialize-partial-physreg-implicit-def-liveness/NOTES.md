# RegisterCoalescer::reMaterializeDef partial-physreg liveness scan ignores DefMI's implicit physreg defs

**File:** llvm/lib/CodeGen/RegisterCoalescer.cpp:1343-1361 (and 1466-1495, 1687-1692)

## Code

```cpp
// In the physical register case, checking that the def is read-undef is not
// enough. We're widening the def and need to avoid clobbering other live
// values in the unused register pieces.
//
// TODO: Targets may support rewriting the rematerialized instruction to only
// touch relevant lanes, in which case we don't need any liveness check.
if (CopyDstReg.isPhysical() && CP.isPartial()) {
  for (MCRegUnit Unit : TRI->regunits(DstReg)) {
    // Ignore the register units we are writing anyway.
    if (is_contained(TRI->regunits(CopyDstReg), Unit))
      continue;

    // Check if the other lanes we are defining are live at the
    // rematerialization point.
    LiveRange &LR = LIS->getRegUnit(Unit);
    if (LR.liveAt(CopyIdx))
      return false;
  }
}
```

## Reasoning

When coalescing `physreg.subX = COPY %vreg` and the source's def `DefMI`
is rematerializable, this loop widens the def of `physreg.subX` into a
def of the full `physreg` (DstReg). To avoid clobbering live data in
the other lanes of `physreg`, it walks every regunit of `DstReg` and
returns false if any non-overlap regunit is live at `CopyIdx`.

The check only examines regunits of the *explicit destination* (`DstReg`).
It does **not** consider implicit physical-register defs that `DefMI`
itself carries — most notably `implicit-def $eflags` from x86 RMW
instructions like `MOV32r0` (`xor eax, eax`), `LEA`, etc. The remat
clone produced later by `Edit.rematerializeAt` faithfully copies those
implicit defs (collected at 1466-1495 into `NewMIImplDefs` and then
applied via `createDeadDef` at 1687-1692 *after* the irrevocable
decision to remat).

Concretely: if at `CopyIdx` `$eflags` is live across the copy point
(produced by an earlier compare and consumed by a later JCC that has
been hoisted just past CopyIdx by some prior coalescing step), the
remat goes through, the new MOV32r0 clobbers $eflags, and the
subsequent createDeadDef is *also* fine with that — it just records
a dead def at the new slot. The consumer of the original $eflags
value still expects it to be live.

The post-hoc dead-def registration silently mints a regunit segment
shorter than the original $eflags live segment, then later regalloc
will assign on the (now-bogus) liveness and the JCC reads stale flags.

## Reproduction sketch

1. Construct an x86 MIR where a vreg whose def is `MOV32r0` (e.g.
   `%v:GR32 = MOV32r0 implicit-def $eflags`) is copied into a
   sub-register of a 64-bit GPR pair, say `$rax = SUBREG_TO_REG 0, %v, %subreg.sub_32bit`
   pre-coalesced into `$rax.sub_32bit = COPY %v`.
2. Sandwich an earlier CMP that defines $eflags, then a JCC immediately
   after the COPY, so $eflags is live across CopyIdx.
3. Run `llc -run-pass=register-coalescer`. The remat at CopyIdx widens
   the def to all of $eax (correct) AND silently clobbers $eflags.

If the partial-physreg/implicit-def combination doesn't reproduce
directly, the same shape can be triggered via the post-RA-greedy path
where Live*Phys-reg defs are introduced by SUBREG_TO_REG-elimination
on x86 GR32→GR64 lowering of small-constant materialisation.

## Severity / kind

Latent miscompile in `register-coalescer`. The hoist of the dead-def
on the implicit physreg happens after the early-exit gate, so the
gate cannot see what's about to be clobbered.

## Fix suggestion

Before lines 1349-1361 (or inside the existing loop), also walk
`DefMI->all_defs()` (or `DefMI->implicit_operands()` filtered to
phys defs) and for each implicit physreg def whose regunit is not
already covered by `regunits(CopyDstReg)`, run the same
`LR.liveAt(CopyIdx)` test and bail if live. This matches the post-hoc
dead-def loop at 1687-1692 in coverage.
