# w51: MachineIRBuilder::buildMaskLowPtrBits silently truncates mask for >64-bit pointers

File: `llvm/lib/CodeGen/GlobalISel/MachineIRBuilder.cpp`
Function: `MachineIRBuilder::buildMaskLowPtrBits` (lines 247-255)

## Bug

```cpp
MachineInstrBuilder MachineIRBuilder::buildMaskLowPtrBits(const DstOp &Res,
                                                          const SrcOp &Op0,
                                                          uint32_t NumBits) {
  LLT PtrTy = Res.getLLTTy(*getMRI());
  LLT MaskTy = LLT::scalar(PtrTy.getSizeInBits());
  Register MaskReg = getMRI()->createGenericVirtualRegister(MaskTy);
  buildConstant(MaskReg, maskTrailingZeros<uint64_t>(NumBits));
  return buildPtrMask(Res, Op0, MaskReg);
}
```

`maskTrailingZeros<uint64_t>(NumBits)` returns a `uint64_t` whose low
`NumBits` are zero and whose upper `64-NumBits` are 1. This value is
then handed to `buildConstant(MaskReg, int64_t)` which calls
`ConstantInt::getSigned(IntN, Val, /*implicitTrunc=*/true)` where
`IntN` has `PtrTy.getSizeInBits()` bits.

For pointer widths > 64 (some custom address spaces, e.g. AMDGPU fat
pointers; also future arches with 128-bit address spaces), the
implicit truncation cap at 64 means that the upper `PtrSize-64` mask
bits are **zero**. The ensuing `G_PTRMASK` then zero-masks the high
portion of the pointer rather than preserving it.

For pointer widths < 64 (e.g. 16-bit / 32-bit DataLayout pointers), the
`getSigned(IntN, Val, /*implicitTrunc=*/true)` truncates the top of the
mask away, which happens to be correct because the high bits of the
mask are all ones and we're keeping only the low PtrSize bits — but
this masks any signed-vs-unsigned interpretation issue, so the helper
"accidentally works" only for PtrSize <= 64.

## Comparison

The sister helper `buildPtrAdd` accepts arbitrary-width offsets via
the LLT builder. `buildMaskLowPtrBits` is the only "mask off low bits
of a pointer" helper and hard-codes the mask source as `uint64_t`.

## Impact

Any future target with >64-bit pointers using this helper to align /
mask the low bits of an address (e.g. `__builtin_align_down`, tag-pointer
stripping, ARMv8.5 MTE-style tagging on a 128-bit segmented model)
will silently zero the upper pointer bits, dereferencing the wrong
object.

On current x86_64 (PtrSize == 64) the helper is correct in practice.

## Fix

```cpp
APInt Mask = APInt::getHighBitsSet(PtrTy.getSizeInBits(),
                                   PtrTy.getSizeInBits() - NumBits);
buildConstant(MaskReg, Mask);
```

## Status

Source-confirmed; latent for x86 today (PtrSize<=64), would surface for
any 128-bit-pointer custom address space. Low severity for x86_64.
