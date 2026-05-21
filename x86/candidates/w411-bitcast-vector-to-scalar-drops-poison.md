# w411 — `bitcast <NxiK> to iN*K` silently zeroes poison source lanes (miscompile)

## Component
`llvm/lib/Analysis/ConstantFolding.cpp` — `foldConstVectorToAPInt` (called from `FoldBitCast`)

## Source citation
`llvm/lib/Analysis/ConstantFolding.cpp:79-107` (focus on lines **93-96**) and the call site at `llvm/lib/Analysis/ConstantFolding.cpp:192-220` (in particular line **217**).

```cpp
// llvm/lib/Analysis/ConstantFolding.cpp:79
static Constant *foldConstVectorToAPInt(APInt &Result, Type *DestTy,
                                        Constant *C, Type *SrcEltTy,
                                        unsigned NumSrcElts,
                                        const DataLayout &DL) {
  unsigned BitShift = DL.getTypeSizeInBits(SrcEltTy);
  for (unsigned i = 0; i != NumSrcElts; ++i) {
    Constant *Element;
    if (DL.isLittleEndian())
      Element = C->getAggregateElement(NumSrcElts - i - 1);
    else
      Element = C->getAggregateElement(i);

    if (isa_and_nonnull<UndefValue>(Element)) {
      Result <<= BitShift;     // <-- BUG: poison/undef silently becomes 0
      continue;
    }
    ...
  }
  return nullptr;
}
```

(Note: `PoisonValue` is a subclass of `UndefValue`, so `isa<UndefValue>(Element)` matches both undef *and* poison.)

The caller (`FoldBitCast`, around line 217) then materialises `ConstantInt::get(DestTy, Result)` — a concrete integer with the poison/undef-derived bits as `0`. The poison status of the source lane is completely lost.

Contrast: the byte-source path at `ConstantFolding.cpp:196-199` correctly bails:
```cpp
// Bitcasting a byte containing any poison bit to an integer or fp type
// yields poison.
if (SrcEltTy->isByteTy() && C->containsPoisonElement())
  return PoisonValue::get(DestTy);
```
But the equivalent *integer*-source path has no such guard — so the bug only manifests for `iN`-element source vectors.

## Reproducer (`/tmp/cf_hunt/bitcast_poison_min.ll`)
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"

define i32 @t_min() {
  %r = bitcast <2 x i16> <i16 0, i16 poison> to i32
  ret i32 %r
}

define i32 @t_min_4xi8() {
  %r = bitcast <4 x i8> <i8 1, i8 poison, i8 3, i8 4> to i32
  ret i32 %r
}

; Sanity check: when ALL lanes are poison, the fold IS correct (poison)
define i32 @t_all_poison_ok() {
  %r = bitcast <2 x i16> <i16 poison, i16 poison> to i32
  ret i32 %r
}
```

Command:
```
opt -passes=instsimplify -S /tmp/cf_hunt/bitcast_poison_min.ll
```

## Actual output
```llvm
define i32 @t_min() {
  ret i32 0                  ; WRONG — should be poison
}

define i32 @t_min_4xi8() {
  ret i32 67305473           ; WRONG — 0x04030001, poison byte silently zeroed
}

define i32 @t_all_poison_ok() {
  ret i32 poison             ; correct
}
```

(For `t_min_4xi8`, `67305473 == 0x04030001`. The poison byte was substituted with `0x00`.)

## Expected output
For both `t_min` and `t_min_4xi8`, the source vector contains a `poison` lane that contributes bits to the result integer; per LangRef ("Poisoned Values"), any instruction whose result depends on a poison operand returns poison. So the expected fold is:
```llvm
define i32 @t_min()      { ret i32 poison }
define i32 @t_min_4xi8() { ret i32 poison }
```

## A second reproducer, vector destination (`<NxiK>` → `<MxiK'>`):

```llvm
define <2 x i32> @t4() {
  %r = bitcast <8 x i8>
        <i8 1, i8 2, i8 3, i8 4, i8 5, i8 poison, i8 7, i8 8>
       to <2 x i32>
  ret <2 x i32> %r
}
```

This triggers the second flavour of the bug in
`FoldBitCast` proper (the vector→vector code path at `ConstantFolding.cpp:341-400`):

```cpp
// 376-385
while (BufferBitSize >= DstBitSize) {
  unsigned ShiftAmt = isLittleEndian ? 0 : BufferBitSize - DstBitSize;
  // Emit undef/poison, if all undef mask fragment bits are set.
  if (UndefMask.extractBits(DstBitSize, ShiftAmt).isAllOnes()) {     // <-- only if ALL bits undef
    if (!PoisonMask.extractBits(DstBitSize, ShiftAmt).isZero()) {
      Result.push_back(PoisonValue::get(DstEltTy));
    } else {
      Result.push_back(UndefValue::get(DstEltTy));
    }
  } else {
    // SOME bits concrete + SOME bits poison -> push a plain ConstantInt,
    // silently dropping the poison information.
    APInt Elt = Buffer.extractBits(DstBitSize, ShiftAmt);
    Result.push_back(ConstantInt::get(DstEltTy, Elt));
  }
  ...
}
```

`PoisonMask` is populated for poison src lanes (`ConstantFolding.cpp:360-361`), but it is only *consulted* inside the all-undef branch (line 379). The "mixed concrete + poison" case at line 388 silently zero-fills the poison fragment and emits a concrete `ConstantInt`. The standalone `PoisonDstElts` post-pass at line 403 only fixes the case where the source element type is a *byte* type (it is only populated by `computePoisonDstLanes`, called at `ConstantFolding.cpp:312-314` under `if (SrcEltTy->isByteTy())`). For integer-source vectors, no such fix-up runs.

Running the second reproducer:
```
$ opt -passes=instsimplify -S
define <2 x i32> @t4() {
  ret <2 x i32> <i32 67305985, i32 134676485>      ; lane 1 is wrong
}
```

`67305985 == 0x04030201` (lane 0, all bytes concrete, OK). `134676485 == 0x08070005` (lane 1, byte index 5 was `poison` and was silently substituted with `0x00`). Expected: lane 1 must be `poison`.

## Why this matters
- LangRef requires `bitcast` of a value that depends on poison to yield poison; otherwise the optimizer is conjuring a concrete-bit pattern out of thin air.
- This is the classic "refinement in the wrong direction": passes that later observe the folded `ConstantInt` (e.g. compare against `0`, `select` simplification, `icmp eq …, 0` -> `true`, range analysis, demanded-bits) will draw conclusions that are not justified by the input program, which is the textbook recipe for a security-relevant miscompile (UB-from-poison silently turning into a CMP-true and removing a guard).
- Both the scalar-destination path (`foldConstVectorToAPInt`, line 93-96) and the vector-destination path (lines 376-390) are affected; the byte-typed paths got the fix-up, the integer-typed paths did not.

## Fix sketch
- In `foldConstVectorToAPInt`, distinguish `isa<PoisonValue>(Element)` from plain `UndefValue` and propagate poison to the whole scalar result by returning `PoisonValue::get(DestTy)`.
- In the main `FoldBitCast` loop (lines 376-390), the existing `PoisonMask` already records the per-bit poison footprint — change the predicate at line 379 from `UndefMask…isAllOnes()` to a two-stage test: first `if (!PoisonMask.extractBits(...).isZero()) push poison; else if (UndefMask…isAllOnes()) push undef; else push concrete`.
- (Alternative) extend `computePoisonDstLanes` / `PoisonDstElts` to be populated for any source-element type, not only byte types.

## Severity
Miscompile: a `bitcast` of a `<NxiK>` constant vector that contains a `poison` lane folds to a *concrete* integer (zero-filled in the poison bits) instead of `poison`. Reproduces with one of LLVM's most fundamental passes (`instsimplify`) on the smallest possible inputs.

## Confidence
High. Two independent code paths exhibit the same defect; reproducers are minimal, deterministic, and bypass `instcombine` entirely.
