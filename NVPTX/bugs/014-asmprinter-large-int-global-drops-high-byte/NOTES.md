# 014 — Top-level large-integer (iN, N>64, N not a multiple of 8) global drops its high partial byte

- **Kind:** miscompile
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1737-1740, 1752-1756 (reached from 1067/1081)  (class `C4-asmprinter-const-emission`)
- **Found in:** round-2 class sweep (sibling of round-1 finds)

## Summary

module-scope `iN` global (N>64, N not a multiple of 8) drops its top partial byte (silent wrong constant)

## Mechanism / root cause

bufferAggregateConstant handles a bare large-integer global via the ExtendBuffer lambda:

    auto ExtendBuffer = [](APInt Val, AggBuffer *Buffer) {
      for (unsigned I : llvm::seq(Val.getBitWidth() / 8))
        Buffer->addByte(Val.extractBitsAsZExtValue(8, I * 8));
    };
    ...
    if (const ConstantInt *CI = dyn_cast<ConstantInt>(CPV)) {
      ExtendBuffer(CI->getValue(), aggBuffer);   // line 1754
      return;
    }

The loop bound is Val.getBitWidth() / 8 using TRUNCATING integer division. For a width that is not a multiple of 8 (e.g. i65, i100), the final partial byte holding the top (bitWidth % 8) bits is never written. The AggBuffer is sized by getTypeStoreSize = ceil(bitWidth/8) (line 1072), so the dropped byte stays 0 in the buffer and the emitted PTX silently zeroes the high bits. Compare bufferLEByte's AddIntToBuffer (lines 1657-1672), which correctly uses NumBytes = (bitWidth+7)/8 and handles the last partial byte separately; that correct path is used for large ints NESTED in arrays/structs, but the top-level large-int global goes through the buggy ExtendBuffer. Verified with built llc: for @g = global i65 u0x1FFFFFFFFFFFFFFFF (= 2^65-1), the emitted PTX is `g[9] = {255,255,255,255,255,255,255,255}` (top byte, value 1, dropped) so the global reads back as 2^64-1 instead of 2^65-1. For i100 value 0xF...A the top nibble byte (15) is dropped. i128/fp128 are unaffected because 128 is a multiple of 8.

## Trigger

Any module-scope global of integer type with bit width > 64 that is not a multiple of 8 and whose top (width%8) bits are nonzero, in addrspace(1) global or addrspace(4) const, with a non-zero/non-undef initializer.

## Reproducer

```
@g = addrspace(1) global i65 u0x1FFFFFFFFFFFFFFFF, align 16
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -o - repro.ll`

## Verification

Reproduced with the built NVPTX `llc` (confirmed_with_llc=True, finder confidence 0.97).
