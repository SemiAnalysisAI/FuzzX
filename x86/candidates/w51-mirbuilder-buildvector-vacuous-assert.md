# w51: MachineIRBuilder G_BUILD_VECTOR / G_BUILD_VECTOR_TRUNC / G_CONCAT_VECTORS vacuous min-size assert

File: `llvm/lib/CodeGen/GlobalISel/MachineIRBuilder.cpp`
Function: `MachineIRBuilder::buildInstr` (lines 1510-1557)

## Bug

Three vector-build cases all carry the same broken assertion that was
intended to enforce "at least 2 source operands":

```cpp
case TargetOpcode::G_BUILD_VECTOR: {
  assert((!SrcOps.empty() || SrcOps.size() < 2) &&
         "Must have at least 2 operands");
  ...
}
case TargetOpcode::G_BUILD_VECTOR_TRUNC: {
  assert((!SrcOps.empty() || SrcOps.size() < 2) &&
         "Must have at least 2 operands");
  ...
}
case TargetOpcode::G_CONCAT_VECTORS: {
  ...
  assert((!SrcOps.empty() || SrcOps.size() < 2) &&
         "Must have at least 2 operands");
  ...
}
```

`(!SrcOps.empty() || SrcOps.size() < 2)` is **always true**:
- `empty()` -> `!empty()=false`, `size()<2`=true  -> true
- `size()==1` -> `!empty()=true` -> true
- `size()>=2` -> `!empty()=true` -> true

The intended check was `!SrcOps.empty() && SrcOps.size() >= 2`
(or equivalently `SrcOps.size() >= 2`). The author wrote the inverse
short-circuit pattern.

## Impact

Pure assertion bug: callers that accidentally pass 0 or 1 source
operands silently pass this guard. For 0 sources the subsequent
"input scalars/vectors do not exactly cover the output vector register"
assertion is the only thing that catches the empty case (and only in
asserts builds). For 1-source `G_CONCAT_VECTORS` (which the verifier
elsewhere considers ill-formed), this assert is the supposed last line
of defense and fails to fire.

`MachineVerifier`'s `visitMachineInstrBefore` checks the cover-size
invariant but does not explicitly forbid 1-source `G_BUILD_VECTOR` /
`G_CONCAT_VECTORS`; combiners that synthesize these via reduce-then-build
patterns can therefore produce single-operand variants that confuse
downstream passes (selection patterns assume a multi-element BV).

## Fix

```cpp
assert(SrcOps.size() >= 2 && "Must have at least 2 operands");
```

## Status

Source-confirmed dead-assertion. No runtime miscompile reproducer; the
bug masks misuse rather than directly miscompiling. Low severity but
trivially confirmable from the source alone.
