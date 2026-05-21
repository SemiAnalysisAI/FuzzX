# LowerTypeTests bitset byte-array load drops `!invariant.load`

## File and root cause

`llvm/lib/Transforms/IPO/LowerTypeTests.cpp` —
`LowerTypeTestsModule::createBitSetTest` (load at line 678).

```cpp
} else {
  Constant *ByteArray = TIL.TheByteArray;
  if (AvoidReuse && !ImportSummary) {
    // Each use of the byte array uses a different alias. ...
    ByteArray = GlobalAlias::create(Int8Ty, 0, GlobalValue::PrivateLinkage,
                                    "bits_use", ByteArray, &M);
  }

  Value *ByteAddr = B.CreateGEP(Int8Ty, ByteArray, BitOffset);
  Value *Byte = B.CreateLoad(Int8Ty, ByteAddr);                   // <-- 678

  Value *ByteAndMask =
      B.CreateAnd(Byte, ConstantExpr::getPtrToInt(TIL.BitMask, Int8Ty));
  return B.CreateICmpNE(ByteAndMask, ConstantInt::get(Int8Ty, 0));
}
```

`TIL.TheByteArray` is the bitset byte array allocated by
`LowerTypeTestsModule::buildBitSetsFromDisjointSet`. Searching from that path
to the actual definition: it is a `private constant` GlobalVariable holding
the CFI bitset (the value of `bits` in the dump below). It is never written.

Despite the bytes being part of a `constant` global, the `CreateLoad` at line
678 produces a `LoadInst` with no metadata.

## Reproducer

`x86/candidates/w441-ltt-bytearray-load-no-invariant.ll` (a verbatim copy of
the in-tree `llvm/test/Transforms/LowerTypeTests/simple.ll`).

### `opt -S -passes=lowertypetests` diff (excerpts)

The pass synthesizes three byte-array bitset tests, each of the form:

```llvm
@1 = private constant [68 x i8] c"\03\00\00..."   ; the byte array
@bits   = private alias i8, ptr @1
@bits_use   = private alias i8, ptr @bits.3
@bits_use.1 = private alias i8, ptr @bits
@bits_use.2 = private alias i8, ptr @bits

...
  %6  = getelementptr i8, ptr @bits_use.2, i32 %3
  %7  = load i8, ptr %6, align 1                      ; <-- no !invariant.load
  %8  = and i8 %7, ptrtoint (ptr inttoptr (i8 1 to ptr) to i8)
  %9  = icmp ne i8 %8, 0
...
  %17 = getelementptr i8, ptr @bits_use.1, i32 %14
  %18 = load i8, ptr %17, align 1                     ; <-- no !invariant.load
  %19 = and i8 %18, ptrtoint (ptr inttoptr (i8 1 to ptr) to i8)
...
  %6  = getelementptr i8, ptr @bits_use, i32 %3
  %7  = load i8, ptr %6, align 1                      ; <-- no !invariant.load
  %8  = and i8 %7, ptrtoint (ptr inttoptr (i8 2 to ptr) to i8)
```

The underlying `@1` is `private constant`, and the chain of aliases preserves
that. These loads are genuinely invariant but the metadata is missing.

## Why this matters

* CFI type-test bitset reads are extremely hot in CFI-instrumented binaries
  (every indirect call site does one). Without `!invariant.load`, downstream
  LICM cannot hoist a redundant bitset load out of a loop when the pointer
  being checked is loop-invariant.
* `GVNHoist` and `EarlyCSE` need either AA or `!invariant.load` to commonize
  bitset reads across CFG paths; AA's `pointsToConstantMemory` proves it for
  some constant globals but breaks down across the `private alias` indirection
  this pass introduces specifically for security hardening (`AvoidReuse` path
  above creates a fresh alias per use, which AA may treat conservatively).
* The fix is a one-liner — attach `!invariant.load` on the `LoadInst`.

## Fix sketch

```cpp
LoadInst *ByteLI = B.CreateLoad(Int8Ty, ByteAddr);
ByteLI->setMetadata(LLVMContext::MD_invariant_load,
                    MDNode::get(M.getContext(), {}));
Value *Byte = ByteLI;
```
