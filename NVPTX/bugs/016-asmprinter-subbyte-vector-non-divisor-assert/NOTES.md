# 016 — Sub-byte vector global whose element width does not evenly divide a byte (e.g. <8 x i3>, <2 x i3>) asserts / report_fatal_errors on valid IR

- **Kind:** assertion (release: graceful fatal_error)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1803-1872 (assert at 1823; fatal_error at 1853-1855)  (class `C4-asmprinter-const-emission`)
- **Found in:** round-2 class sweep (sibling of round-1 finds)

## Summary

vector global with sub-byte element width not dividing 8 (`<2 x i3>`, `<8 x i5>`) asserts in NVPTXAsmPrinter

## Mechanism / root cause

bufferAggregateConstVec takes the sub-byte merge path whenever BuffSize (store size in bytes) < NumElems. It then assumes the element size evenly divides a byte:

    unsigned ElemTySize = ElemTy->getPrimitiveSizeInBits();
    assert(ElemTySize < 8 && "Expected sub-byte data type.");
    assert(8 % ElemTySize == 0 && "Element type size must evenly divide a byte.");
    unsigned NumElemsPerByte = 8 / ElemTySize;

For a vector with a sub-byte element width that does not divide 8 (i3, i5, i6, i7), the assert fires in an assertions build. In a release build the assert is gone: NumElemsPerByte = 8/3 = 2, and ConvertSubCVtoInt8 bitcasts a <2 x i3> (6 bits) to i8 (8 bits); ConstantFoldConstant returns null for the mismatched-size bitcast, hitting report_fatal_error("Cannot lower vector global with unusual element type") at line 1853-1855. Either way valid IR (sub-byte vector globals are accepted by the verifier) cannot be lowered. Verified with built llc: both `<8 x i3>` and `<2 x i3>` initialized globals hit the line-1823 assertion. This is a sibling of the <8 x i4> packing work: the packing logic only handles the case where element size divides 8 and the buffer/store-size accounting only lines up in that case.

## Trigger

A module-scope addrspace(1)/addrspace(4) global of a fixed vector type whose element is an integer narrower than a byte but whose width does not divide 8 (i3, i5, i6, i7), with a non-zero constant initializer.

## Reproducer

```
@g = addrspace(1) global <2 x i3> <i3 1, i3 2>, align 1
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -o - repro.ll`

## Verification

Reproduced with the built NVPTX `llc` (confirmed_with_llc=True, finder confidence 0.9).
