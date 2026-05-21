# w630: ScheduleDAGInstrs::buildSchedGraph - invariant load bypasses BarrierChain dependency

## Status
NEGATIVE / by-design. Documented for completeness. No miscompile triggered on
x86 with available targets and the candidate IR patterns produced sound code
(see "Triggers attempted" below). The semantics of `!invariant.load` permit
reordering across barriers because the value at the load address is, by
definition, constant for the duration of the function.

## What the code does

`llvm/lib/CodeGen/ScheduleDAGInstrs.cpp:955`
```
// If it's not a store or a variant load, we're done.
if (!MI.mayStore() &&
    !(MI.mayLoad() && !MI.isDereferenceableInvariantLoad()))
  continue;

// Always add dependecy edge to BarrierChain if present.
if (BarrierChain)
  BarrierChain->addPredBarrier(SU);
```

`continue` at line 957-958 fires for dereferenceable invariant loads. That
short-circuits both:

1. The `BarrierChain->addPredBarrier(SU)` call at line 961-962 (so the
   invariant load gets NO chain edge to a preceding `isGlobalMemoryObject`
   barrier).
2. Insertion into the `Stores`/`Loads`/`NonAliasStores`/`NonAliasLoads`
   value maps, so later instructions/barriers also do not see this SU when
   they call `addChainDependencies` or `addBarrierChain`.

The matching block at line 920-938 makes the symmetric assumption: when a
new `isGlobalMemoryObject` SU is created, it calls
`addBarrierChain(Stores)/Loads/...` to install barrier predecessors only
on memops that were entered in those maps. Invariant loads are absent, so
they are silently skipped.

`llvm/lib/CodeGen/TargetInstrInfo.cpp:2231`
```
bool TargetInstrInfo::isGlobalMemoryObject(const MachineInstr *MI) const {
  return MI->isCall() || MI->hasUnmodeledSideEffects() ||
         (MI->hasOrderedMemoryRef() && !MI->isDereferenceableInvariantLoad());
}
```

`llvm/lib/CodeGen/MachineInstr.cpp:1626 isDereferenceableInvariantLoad()`
returns true when every MMO is unordered, not-a-store, and either MOInvariant
+ MODereferenceable, or backed by a constant `PseudoSourceValue`. So a load
carrying `!invariant.load` and properly marked dereferenceable can be moved
freely past atomics/fences/calls from the scheduler's point of view.

## Why this is intentional

LangRef on `!invariant.load`:
> "The optimizer and code generator may assume that the memory location
> referenced by the load contains the same value at all points in the
> program where the memory location is dereferenceable."

If the user puts `!invariant.load` on a load, the value is invariant for
the function's lifetime, so reordering across a barrier is semantically
equivalent. The dependency-graph bypass is the codification of that
assumption.

## Triggers attempted

All produced correct/expected x86 code with the scheduler:

```
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(ptr noalias %inv1, ptr noalias %inv2, ptr noalias %store_dst, i32 %sval) {
entry:
  store i32 %sval, ptr %store_dst, align 4
  fence seq_cst
  %v1 = load i32, ptr %inv1, align 4, !invariant.load !0
  %v2 = load i32, ptr %inv2, align 4, !invariant.load !0
  %r = add i32 %v1, %v2
  ret i32 %r
}
!0 = !{}
```

`llc -mtriple=x86_64-unknown-linux-gnu -mcpu=znver5 -O2 -stop-after=post-RA-sched`
produced:
```
MOV32mr ... store_dst
OR32mi8Locked $rsp, ...     ; x86 fence lowering
MOV32rm ... :: (invariant load (s32) from %ir.inv1)
ADD32rm ... :: (invariant load (s32) from %ir.inv2)
```

Order is preserved relative to the IR. The scheduler did not exploit the
freedom that line 956-958 grants it on this case (heuristics and CPU model
weighting kept the natural order). Stronger fuzzing or targets with very
different latencies would be needed to actually expose a visible reorder.

## Real-bug signature to look for

A miscompile in this area would require all of:
- A load whose MMO has MOInvariant + MODereferenceable
- A subsequent or preceding `isGlobalMemoryObject` (call, side-effect, or
  ordered MMO)
- The user accidentally marked the load `!invariant.load` even though the
  address may actually be modified (e.g. through a different alias, or by
  the called function)

In that case, the scheduler would reorder the load with no warning. The bug
would be in whichever frontend / pass attached `!invariant.load`, not in
the scheduler — but the scheduler swallows the freedom silently with no
sanity check (no assert that the underlying memory is truly invariant when
crossed by a barrier).

## Source citations

- `llvm/lib/CodeGen/ScheduleDAGInstrs.cpp:955-962` — the bypass.
- `llvm/lib/CodeGen/ScheduleDAGInstrs.cpp:920-938` — barrier processing
  that never sees invariant loads.
- `llvm/lib/CodeGen/MachineInstr.cpp:1626-1660` — `isDereferenceableInvariantLoad`.
- `llvm/lib/CodeGen/TargetInstrInfo.cpp:2231-2234` — `isGlobalMemoryObject`.
