# w656: `areContiguous` uses `SizeBits/8`, mis-merges non-byte-multiple integer compares

- **File:** `llvm/lib/Transforms/Scalar/MergeICmps.cpp`
- **Target:** x86_64, LLVM 23.0.0git, default `-O2` lowering path
- **Pass:** `mergeicmps`
- **Severity:** Miscompile (memcmp covers fewer bytes than the source compares; overlapping byte read for one side; high bits of second-half source `i17` are silently dropped)

## Root cause

In `mergeComparisons` the byte-count for the synthesized `memcmp` is
`TotalSizeBits / 8` (integer division), and `areContiguous` uses
`SizeBits / 8` as the per-block stride.

`MergeICmps.cpp:427-432`:

```cpp
static bool areContiguous(const BCECmpBlock &First, const BCECmpBlock &Second) {
  return First.Lhs().BaseId == Second.Lhs().BaseId &&
         First.Rhs().BaseId == Second.Rhs().BaseId &&
         First.Lhs().Offset + First.SizeBits() / 8 == Second.Lhs().Offset &&
         First.Rhs().Offset + First.SizeBits() / 8 == Second.Rhs().Offset;
}
```

`SizeBits` is set from `getTypeSizeInBits` for the icmp operand
(`MergeICmps.cpp:332-334`):

```cpp
return BCECmp(std::move(Lhs), std::move(Rhs),
              DL.getTypeSizeInBits(CmpI->getOperand(0)->getType()), CmpI);
```

There is no `assert(SizeBits % 8 == 0)` and no filter rejecting
non-byte-multiple integer types in `visitICmpLoadOperand`. So:

* `i17` → `SizeBits = 17`, `SizeBits/8 = 2`.
* `i9`  → `SizeBits/8 = 1`.
* `i1`  → `SizeBits/8 = 0` (would let two same-offset i1 loads be considered "contiguous" with themselves).

This is then turned into a memcmp of `TotalSizeBits / 8` bytes
(`MergeICmps.cpp:691-704`):

```cpp
const unsigned TotalSizeBits = std::accumulate(
    Comparisons.begin(), Comparisons.end(), 0u,
    [](int Size, const BCECmpBlock &C) { return Size + C.SizeBits(); });
...
Value *const MemCmpCall = emitMemCmp(
    Lhs, Rhs,
    ConstantInt::get(Builder.getIntNTy(SizeTBits), TotalSizeBits / 8),
    Builder, DL, &TLI);
```

Truncation losses compound: for two i17 loads at offsets 0 and 2,
contiguity passes (`0 + 2 == 2`), then `Total = 34`, `Total/8 = 4`.
The byte range actually read by the original chain is bytes 0..4 (5
bytes), and the *upper bit* of each i17 load is significant — yet the
merged memcmp:

1. Reads only bytes 0..3.
2. Reads byte 2 once (the original load chain accessed it twice — once
   as the high byte of the first i17, once as the low byte of the second
   i17), so the *high* byte of the second load (byte 4) is never
   compared.
3. Compares whole bytes including the 15 bits of padding above the
   `i17` value that the original loads did not load.

The original IR:

```
load i17 @ off 0   → loads bits [0..16]                 (3 bytes touched: 0,1,2-low)
load i17 @ off +2  → loads bits [16..32] from bytes 2..4 (3 bytes touched: 2-low,3,4-low)
```

After merge: `memcmp(a, b, 4)` — compares bytes 0..3 of memory in full.

That is *not* equivalent: differences in bit 16 of either load (i.e.
the low bit of byte 2 of the first load, and the low bit of byte 4 of
the second) are silently ignored or aliased onto a different bit
position.

## Repro

`/tmp/mergeicmps/i17_observable.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i1 @eq(ptr dereferenceable(16) %a, ptr dereferenceable(16) %b) {
entry:
  %a0 = load i17, ptr %a, align 4
  %b0 = load i17, ptr %b, align 4
  %c0 = icmp eq i17 %a0, %b0
  br i1 %c0, label %rhs, label %end

rhs:
  %ag1 = getelementptr inbounds i8, ptr %a, i64 2
  %bg1 = getelementptr inbounds i8, ptr %b, i64 2
  %a1 = load i17, ptr %ag1, align 4
  %b1 = load i17, ptr %bg1, align 4
  %c1 = icmp eq i17 %a1, %b1
  br label %end

end:
  %r = phi i1 [ false, %entry ], [ %c1, %rhs ]
  ret i1 %r
}
```

## Diff (`opt -passes=mergeicmps -S`)

```
-  %a0 = load i17, ptr %a, align 4
-  %b0 = load i17, ptr %b, align 4
-  ...
-  %a1 = load i17, ptr %ag1, align 4
-  %b1 = load i17, ptr %bg1, align 4
-  %c1 = icmp eq i17 %a1, %b1
+  %memcmp = call i32 @memcmp(ptr %a, ptr %b, i64 4)
+  %0 = icmp eq i32 %memcmp, 0
```

The 4 in `memcmp(%a, %b, i64 4)` is the bug signature:
- Original byte coverage: 5 (bytes 0..4).
- Merged byte coverage: 4 (bytes 0..3).
- Original ignores padding above the i17; merged compares it.

## Suggested fix

In `visitICmpLoadOperand` (or in `visitICmp` at `MergeICmps.cpp:332-334`),
reject types whose store size differs from the value bit-width, or
where the type bit-width is not a multiple of 8:

```cpp
Type *Ty = CmpI->getOperand(0)->getType();
TypeSize Bits = DL.getTypeSizeInBits(Ty);
if (Bits.isScalable() || Bits.getFixedValue() % 8 != 0 ||
    Bits != DL.getTypeStoreSizeInBits(Ty))
  return std::nullopt;
```

Alternatively, use `DL.getTypeStoreSize(...) * 8` everywhere instead of
`getTypeSizeInBits()` so stride and length are byte-accurate, but the
"only compare value bits" semantics still need explicit handling.

## Status

Confirmed via `opt -passes=mergeicmps -S` on `i17_observable.ll`: the
pass *does* merge the two i17 comparisons into a single
`memcmp(_, _, 4)`. Filed as potential miscompile; the test case as-is
isn't directly observable without a specific input that toggles the
"silenced" bits, but the IR-level invariant (compare the same bits) is
violated.

Source references from `llvm/lib/Transforms/Scalar/MergeICmps.cpp` (main).
