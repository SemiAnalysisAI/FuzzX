# 011 — Integer/FP vector splat path in bufferAggregateConstant ignores sub-byte packing (overflow/wrong layout for splat ConstantInt of sub-byte vector)

- **Kind:** crash (OOB write)
- **Reachable via:** needs -use-constant-int-for-fixed-length-splat
- **Component:** NVPTXAsmPrinter.cpp 1742-1749  (region `A1-asmprinter-constants`)
- **Candidate id:** c013

## Summary

sub-byte vector splat ConstantInt overflows the AsmPrinter constant buffer (latent path)

## Mechanism / root cause

When a vector constant is represented as a native splat ConstantInt/ConstantFP (i.e. `isa<ConstantInt,ConstantFP>(CPV)` is true and the type is a FixedVectorType), bufferAggregateConstant iterates VTy->getNumElements() and calls bufferLEByte on each scalar element (lines 1745-1746). For a sub-byte element type like <8 x i4>, getAggregateElement returns a scalar i4 ConstantInt for each lane, and bufferLEByte->AddIntToBuffer emits one byte per i4 (NumBytes=(4+7)/8=1), producing 8 bytes for a 4-byte vector. Unlike bufferAggregateConstVec, this path has no sub-byte packing, so it both produces a wrong (unpacked) layout and overflows the AggBuffer (addByte assert `curpos < Size` / OOB write in release). Confirmed crashing: `Assertion failed: (curpos < Size), function addByte`. NOTE: by default LLVM materializes fixed-length splats as ConstantVector (UseConstantIntForFixedLengthSplat defaults to false), so reaching this path through llc requires the hidden flag -use-constant-int-for-fixed-length-splat; hence lower confidence/severity, but the code path is latent and would also be hit by any frontend/transform that constructs splat ConstantInts directly for sub-byte vectors.

## Trigger

nvptx target plus the hidden flag -use-constant-int-for-fixed-length-splat (or IR constructed in-memory with a splat ConstantInt of a sub-byte vector type). A global initialized to a sub-byte vector splat, e.g. <8 x i4> splat (i4 3).

## Reproducer

See `repro.ll` / `cmd.sh`.

```
@g = addrspace(1) global <8 x i4> splat (i4 3)
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -use-constant-int-for-fixed-length-splat -o - repro.ll
```

## Observed (wrong) output

```
Assertion failed: (curpos < Size), function addByte, file NVPTXAsmPrinter.h, line 125.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -use-constant-int-for-fixed-length-splat c013.ll -o -
 #7 llvm::NVPTXAsmPrinter::AggBuffer::addBytes(unsigned char const*, unsigned int, unsigned int)
 #8 llvm::NVPTXAsmPrinter::bufferLEByte(...)::$_0::operator()(llvm::APInt const&) const
 #9 llvm::NVPTXAsmPrinter::bufferAggregateConstant(llvm::Constant const*, llvm::NVPTXAsmPrinter::AggBuffer*)
#10 llvm::NVPTXAsmPrinter::printModuleLevelGV(...)
#11 llvm::NVPTXAsmPrinter::emitGlobals(llvm::Module const&)
#12 llvm::NVPTXAsmPrinter::doFinalization(llvm::Module&)
```

## Expected

llc should emit the correctly sub-byte-packed 4-byte global, identical to the non-flag path: ".visible .global .align 4 .b8 g[4] = {51, 51, 51, 51};" (each byte 0x33 = two packed i4 lanes of value 3). Instead it asserts/overflows because the splat ConstantInt path buffers one byte per i4 lane (8 bytes) into a 4-byte buffer, with no sub-byte packing.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.4, verify confidence 0.95).

> Confirmed the cited mechanism in source and reproduced the assertion with the built llc on valid IR.

Source (NVPTXAsmPrinter.cpp:1742-1749): the "Integer or floating point vector splats" branch in bufferAggregateConstant iterates VTy->getNumElements() and calls bufferLEByte(CPV->getAggregateElement(I), 0, aggBuffer) per lane. For <8 x i4>, each lane is a scalar i4 ConstantInt; bufferLEByte's AddIntToBuffer lambda (lines 1657-1672) computes NumBytes=(4+7)/8=1 and emits 1 byte per lane = 8 bytes total. The AggBuffer is constructed with Size = DL.getTypeStoreSize(ETy) = 4 (line 1080). addByte (NVPTXAsmPrinter.h:124-127) asserts curpos < Size and writes buffer[curpos] into a length-4 SmallVector; the 5th byte trips the assert (debug) / overflows the buffer (release). This path lacks the sub-byte packing that bufferAggregateConstVec (lines 1803-1872) performs.

Reproduced: with -use-constant-int-for-fixed-length-splat, llc aborts with "Assertion failed: (curpos < Size), function addByte, file NVPTXAsmPrinter.h, line 125." The stack trace matches the claimed path exactly: bufferAggregateConstant -> bufferLEByte::$_0 (AddIntToBuffer) -> addBytes -> addByte.

Adversarial checks all pass: (1) the IR is valid and passes opt -verify (exit 0); (2) the flag only changes the in-memory constant representation (native ConstantInt vector splat), NOT IR semantics -- without the flag the same global compiles correctly to g[4]={51,51,51,51} (0x33 per byte = two packed i4 lanes of value 3), so t
