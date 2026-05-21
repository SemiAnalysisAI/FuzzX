# GISel loadi16 / loadi32 ignore -x86-promote-anyext-load flag

**File:** llvm/lib/Target/X86/X86InstrFragments.td (lines 659-695)
**Driver flag:** `-x86-promote-anyext-load` (default true, defined in X86ISelDAGToDAG.cpp:47)

## Symptom

The PatFrag predicates `loadi16` and `loadi32` are defined with two
parallel matchers:

* C++ DAG predicate (used by SelectionDAG): gates anyext-load widening on
  `EnablePromoteAnyextLoad` AND alignment AND `isSimple()`.
* `GISelPredicateCode` (used by GlobalISel): gates the same widening on
  alignment AND `isSimple()` only — the `EnablePromoteAnyextLoad`
  cl::opt is silently ignored.

```td
def loadi32 : PatFrag<(ops node:$ptr), (i32 (unindexedload node:$ptr)), [{
  LoadSDNode *LD = cast<LoadSDNode>(N);
  ISD::LoadExtType ExtType = LD->getExtensionType();
  if (ExtType == ISD::NON_EXTLOAD)
    return true;
  if (ExtType == ISD::EXTLOAD && EnablePromoteAnyextLoad)
    return LD->getAlign() >= 4 && LD->isSimple();
  return false;
}]> {
  let GISelPredicateCode = [{
    auto &Load = cast<GLoad>(MI);
    LLT Ty = MRI.getType(Load.getDstReg());
    if (Load.getMemSizeInBits() == Ty.getSizeInBits())
       return true;
    return Load.getAlign() >= 4 && Load.isSimple();   // <-- no EnablePromoteAnyextLoad guard
  }];
}
```

## Why this matters

* `-x86-promote-anyext-load=false` is a documented escape hatch
  (cl::Hidden but live) that users can flip when the widening is
  semantically problematic (e.g., the wider load may straddle a page
  whose tail bytes are unmapped or have side-effecting AA-visibility).
* On the DAG path the flag disables the widening for any anyext load.
* On the GlobalISel path (`-global-isel`) the flag has no effect — the
  widening still happens. Two paths give divergent assembly for the
  same flag, even though they're meant to share the same predicate.

## Suggested triage

Either:

1. Reference `X86::EnablePromoteAnyextLoad` from the
   `GISelPredicateCode` (and expose it as `extern` if not already), or
2. Mirror the cl::opt in the GISel helper, or
3. Document this divergence and rename / split the predicate.
