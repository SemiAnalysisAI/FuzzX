# w42: SwitchToLookupTable emits switch.load without dereferenceable / nonnull / range metadata

## File
`llvm/lib/Transforms/Utils/SimplifyCFG.cpp`

## Functions
- `SwitchReplacement::replaceSwitch` (LookupTableKind), lines 6968-6993
- `simplifySwitchLookup`, lines 7203-7549

## Description
When the switch lowering builds a private constant array and replaces the PHI
with a loaded value, the load is emitted unconditionally with no metadata:

```cpp
return Builder.CreateLoad(ArrayTy->getElementType(), GEP, "switch.load");
```

Information that the table is fully populated and known at compile time is
available:
- The constant array is a `PrivateLinkage` GlobalVariable with `unnamed_addr`,
  fully initialized (poison entries only when `AllHolesArePoison`).
- The element constants are known and `validLookupTableConstant`-vetted.
- The load executes only when the range check passes (or always, when the
  table is `GeneratingCoveredLookupTable`).

The emitted load therefore could legally carry:
- `!dereferenceable !{i64 sizeof(elt)}` (the GEP is always in-bounds inside the
  global),
- `!invariant.load !{}` (the table is constant),
- `!range !{...}` derived from the min/max integer constants in the table,
- `!nonnull !{}` when every pointer entry is provably nonnull
  (e.g. all entries are `getelementptr` of a global or are non-null globals).

Status: source-confirmed missed optimization. Not a soundness bug. The lost
metadata can be observed by inspecting the IR for any switch whose cases set
a PHI to a small fixed set of non-null function pointers and where
`-passes=simplifycfg` produces a `switch.load` with no `!nonnull` /
`!dereferenceable` / `!invariant.load`.

Not filing as a confirmed bug under this hunt's rules; documented for the
candidate pool. Listed for completeness because the task description names
this exact pattern.
