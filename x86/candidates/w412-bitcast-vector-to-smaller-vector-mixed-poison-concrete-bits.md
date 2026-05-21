# w412 — `bitcast <NxiK> to <MxiL>` silently zeroes poison source bits when the resulting lane is a mix of poison and concrete bits

## Component
`llvm/lib/Analysis/ConstantFolding.cpp` — main loop of `FoldBitCast` (the vector-to-vector path)

## Source citation
`llvm/lib/Analysis/ConstantFolding.cpp:376-400`:

```cpp
// Create DstElts from Buffer.
while (BufferBitSize >= DstBitSize) {
  unsigned ShiftAmt = isLittleEndian ? 0 : BufferBitSize - DstBitSize;
  // Emit undef/poison, if all undef mask fragment bits are set.
  if (UndefMask.extractBits(DstBitSize, ShiftAmt).isAllOnes()) {     // <-- only fires when ALL bits are undef
    // Push poison, if any bit in poison mask fragment is set.
    if (!PoisonMask.extractBits(DstBitSize, ShiftAmt).isZero()) {
      Result.push_back(PoisonValue::get(DstEltTy));
    } else {
      Result.push_back(UndefValue::get(DstEltTy));
    }
  } else {
    // Create and push DstElt.
    APInt Elt = Buffer.extractBits(DstBitSize, ShiftAmt);
    Result.push_back(ConstantInt::get(DstEltTy, Elt));       // <-- BUG: drops PoisonMask when mixed
  }

  // Shift unused Buffer fragment to lower bits.
  ...
}
```

`PoisonMask` is populated correctly at line 360-361 for each `PoisonValue` source lane, but it is *only consulted inside the all-undef branch* (line 379). When a destination lane fragment is a mix of concrete and poison source bits — for example `bitcast <2 x i16> <i16 0, i16 poison> to i32`, where lane 0 contributes the low 16 bits (concrete `0x0000`) and lane 1 contributes the high 16 bits (poison) — the `UndefMask…isAllOnes()` predicate is false, the `else` branch is taken, and a plain `ConstantInt` is materialised with the poison bits substituted by zero.

The dedicated `PoisonDstElts` repair pass at lines 311-314, 403-404 *does* the right thing — but it is only populated for *byte*-element source vectors (`if (SrcEltTy->isByteTy())` at line 312, via `computePoisonDstLanes`). For integer- or FP-element source vectors, no such fix-up runs, so the silent zero-fill ships to the caller.

The same defect appears in the sibling `foldConstVectorToAPInt` (vector-to-scalar path; cited separately in w411).

## Reproducer (`/tmp/cf_hunt/bitcast_poison.ll`)
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"

; <8 x i8> -> <2 x i32> with a single poison i8 lane
define <2 x i32> @t4() {
  %r = bitcast <8 x i8>
        <i8 1, i8 2, i8 3, i8 4, i8 5, i8 poison, i8 7, i8 8>
       to <2 x i32>
  ret <2 x i32> %r
}

; Same shape, different element type
define <4 x i16> @t1() {
  %r = bitcast <2 x i32> <i32 poison, i32 305419896> to <4 x i16>
  ret <4 x i16> %r
}
```

Command:
```
opt -passes=instsimplify -S /tmp/cf_hunt/bitcast_poison.ll
```

## Actual output
```llvm
define <2 x i32> @t4() {
  ret <2 x i32> <i32 67305985, i32 134676485>
}

define <4 x i16> @t1() {
  ret <4 x i16> <i16 poison, i16 poison, i16 22136, i16 4660>
}
```

`67305985 = 0x04030201` — lane 0 is fine, all source bytes concrete.
`134676485 = 0x08070005` — lane 1 is WRONG: the byte at offset 5 was `poison`, but it was silently substituted with `0x00`. The resulting i32 is presented to the rest of the optimizer as a concrete value `0x08070005`, *not* as poison.

`t1` is the **opposite direction** to t4 (i32→i16, big-to-small in element count): one whole source i32 lane is poison, contributing 32 bits to two consecutive i16 dst lanes. The folder *does* mark both i16 lanes as poison here — because for this lane all dst-bit fragments come entirely from the poison source lane, so `UndefMask…isAllOnes()` is true for each. So `t1` is the "lucky" case where the broken predicate happens to be true; `t4` is the unlucky case where the predicate is false.

The bug is reachable with **either lane width ratio**, as long as a destination lane is composed of both poison and concrete source bits.

## Expected output
```llvm
define <2 x i32> @t4() {
  ret <2 x i32> <i32 67305985, i32 poison>     ; lane 1 must be poison
}
```

## Why this matters (downstream)
Just like w411, a follow-on `icmp` / `select` / `and` / `or` will now consume a *concrete* integer where the IR semantics demand *poison*, allowing the optimizer to draw conclusions (and remove guards / branches) that the source program does not justify. Worked example:

```llvm
define i1 @bad() {
  %v  = bitcast <8 x i8> <i8 1, i8 2, i8 3, i8 4, i8 5, i8 poison, i8 7, i8 8>
        to <2 x i32>
  %x  = extractelement <2 x i32> %v, i32 1
  %c  = icmp eq i32 %x, 0x08070005
  ret i1 %c
}
```

`%c` ought to be `poison` (the bitcast result lane 1 is dependent on a poison byte). The folder will fold this whole function to `ret i1 true`, treating the comparison as definitively true and likely letting subsequent passes optimise away the surrounding control flow.

## Fix sketch
At line 376-389, restructure the predicate so poison is checked *first*, independently of the all-undef predicate. Concretely:

```cpp
APInt PMFrag  = PoisonMask.extractBits(DstBitSize, ShiftAmt);
APInt UMFrag  = UndefMask.extractBits(DstBitSize, ShiftAmt);
if (!PMFrag.isZero()) {
  Result.push_back(PoisonValue::get(DstEltTy));
} else if (UMFrag.isAllOnes()) {
  Result.push_back(UndefValue::get(DstEltTy));
} else {
  APInt Elt = Buffer.extractBits(DstBitSize, ShiftAmt);
  Result.push_back(ConstantInt::get(DstEltTy, Elt));
}
```

This restores the invariant that any poison bit in a destination lane forces that whole lane to poison (since a `ConstantInt` cannot represent "this byte is poison, this byte is concrete"). The dedicated post-pass at line 403 then becomes redundant for the integer-source case.

## Severity
Same family as w411 — silent miscompile in `FoldBitCast`. Distinct code path (vector-to-vector vs vector-to-scalar), distinct reproducer, same root cause (poison check gated by all-undef predicate). Trigger is one of the most fundamental simplification passes (`instsimplify`/`instcombine`) on a trivial input.

## Confidence
High. The bug is structural (the existing `PoisonMask` is computed correctly and ignored), the fix is local (re-order the three branches), the reproducer is minimal and exhibits the wrong concrete-value behaviour deterministically.
