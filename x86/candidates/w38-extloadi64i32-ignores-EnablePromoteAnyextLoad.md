# extloadi64i32 widens narrow EXTLOAD to i32 without checking -x86-promote-anyext-load

**File:** llvm/lib/Target/X86/X86InstrFragments.td (lines 743-752)
**Driver flag:** `-x86-promote-anyext-load` (default true, X86ISelDAGToDAG.cpp:47)

## The predicate

```td
def extloadi64i32  : PatFrag<(ops node:$ptr), (i64 (unindexedload node:$ptr)), [{
  LoadSDNode *LD = cast<LoadSDNode>(N);
  ISD::LoadExtType ExtType = LD->getExtensionType();
  if (ExtType != ISD::EXTLOAD)
    return false;
  if (LD->getMemoryVT() == MVT::i32)
    return true;
  return LD->getAlign() >= 4 && LD->isSimple();
}]>;
```

The third branch (`return LD->getAlign() >= 4 && LD->isSimple();`)
fires when `MemoryVT` is i8 or i16 with an anyext to i64.  It then
widens the memory access to 32 bits — i.e., the resulting machine
load reads 4 bytes from `$ptr` even though the source IR loaded 1 or
2 bytes.

This is the same anyext-widening that sibling `loadi16` and `loadi32`
do, but those two predicates additionally gate on
`EnablePromoteAnyextLoad` (the `-x86-promote-anyext-load` cl::opt).
`extloadi64i32` does not, so even when the flag is set to `false` to
disable anyext-widening, this PatFrag still widens.

## Why this matters

* `-x86-promote-anyext-load=false` is the documented escape hatch for
  cases where widening introduces semantic problems (spurious faults
  on a 1-byte load at an end-of-mapping address that has been mmap'd
  with only 1 byte readable, etc.). On platforms / addresses where the
  widening is unsound, users currently can't disable it for the
  anyext-i64-from-i8/i16 path.
* The DAG paths via `loadi16` and `loadi32` are consistent with each
  other but `extloadi64i32` is the odd one out.

## Suggested fix

Add the `&& EnablePromoteAnyextLoad` guard to the third branch:

```cpp
  return EnablePromoteAnyextLoad && LD->getAlign() >= 4 && LD->isSimple();
```

(matches the pattern in `loadi16`/`loadi32` exactly.)
