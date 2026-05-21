# w51: CallLowering sret-demote pointer inherits return-value attributes (signext/zeroext/etc.)

File: `llvm/lib/CodeGen/GlobalISel/CallLowering.cpp`
Functions:
- `CallLowering::insertSRetIncomingArgument` (lines 1081-1101)
- `CallLowering::insertSRetOutgoingArgument` (lines 1103-1123)

## Bug

When a return value is too large to fit in registers, GISel demotes it
to an implicit pointer argument and the callee stores the return value
through that pointer. Both helpers build the demote `ArgInfo` like:

```cpp
ArgInfo DemoteArg(DemoteReg, ValueVTs[0].getTypeForEVT(...),
                  ArgInfo::NoArgIndex);
setArgFlags(DemoteArg, AttributeList::ReturnIndex, DL, F);  // <-- bug
DemoteArg.Flags[0].setSRet();
```

`setArgFlags(..., AttributeList::ReturnIndex, ...)` is `addArgFlagsFromAttributes`
which calls `addFlagsFromAttrSet(Flags, Attrs.getAttributes(ReturnIndex))`.
The set of flags translated includes `SExt`, `ZExt`, `InReg`, `Nest`,
`ByVal`, `ByRef`, `Preallocated`, `InAlloca`, `Returned`, `SwiftSelf`,
`SwiftAsync`, `SwiftError`.

So if the user's IR is

```
declare signext { i64, i64, i64 } @big() ; signext on return value
```

the demote pointer-typed sret arg ends up with `Flags.SExt = true`. The
target CC machinery may then sign-extend the SRet pointer (or, more
subtly, pick a different reg class / location from the CC table that
keyed on SExt). On i686 with stdcall/regcall it could end up routed to
a different register.

For the outgoing side (`insertSRetOutgoingArgument`), `setArgFlags(...,
ReturnIndex, ..., CB)` is even worse: it pulls `RetAttrs` of the call,
which can carry `NoAlias`, `NonNull`, `Dereferenceable` — those are then
silently ignored, BUT `ZExt`/`SExt` are *not* ignored. A `call zeroext
{i64,i64,i64} @foo()` produces a demote pointer ArgInfo with both
`ZExt=true` and `SRet=true`.

## Comparison

`SelectionDAGBuilder` builds the SRet demote arg flags freshly (with
just `setSRet`); it does NOT copy the return's SExt/ZExt attrs onto the
pointer. The GISel path is the outlier.

## Impact

For the common SysV-x86_64 case the byval/sret machinery doesn't
consult SExt/ZExt for pointer args, so a miscompile may not surface.
But on 32-bit i686 / Win64 / regcall / nest, the CC tables do branch on
SExt/ZExt/InReg, and an sret demote pointer flagged SExt may be assigned
to a sign-extended location, mismatching the caller's view.

## Fix

Either pass `ArgInfo::NoArgIndex` /nothing to setArgFlags (constructing
the Flags manually with just SRet + alignment), or clear extension flags
after the call:

```cpp
DemoteArg.Flags[0].setSRet();
DemoteArg.Flags[0].setSExt(false);
DemoteArg.Flags[0].setZExt(false);
// ... or skip setArgFlags entirely for the demote arg
```

## Status

Source-confirmed structural mismatch with DAG; concrete miscompile would
need a non-trivial CC + return-attr combination (regcall return signext
{i64,...} on i686 would be the place to look). No runtime repro produced.
