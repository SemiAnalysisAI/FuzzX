# X86PartialReduction::trySADReplacement uses Op0 in place of Op1 when splitting

File: llvm/lib/Target/X86/X86PartialReduction.cpp:310-317

```
for (unsigned i = 0; i != NumSplits; ++i) {
  SmallVector<int, 64> ExtractMask(IntrinsicNumElts);
  std::iota(ExtractMask.begin(), ExtractMask.end(), i * IntrinsicNumElts);
  Value *ExtractOp0 = Builder.CreateShuffleVector(Op0, Op0, ExtractMask);
  Value *ExtractOp1 = Builder.CreateShuffleVector(Op1, Op0, ExtractMask);   // <-- BUG: should be (Op1, Op1)
  Ops[i] = Builder.CreateCall(PSADBWFn, {ExtractOp0, ExtractOp1});
  ...
}
```

## Reasoning

`ExtractOp1` is constructed via `CreateShuffleVector(Op1, Op0, ExtractMask)`. The
intent (as for `ExtractOp0` on the line above) is a self-shuffle that extracts
the i'th lane group of `Op1`. Because the second operand of the shuffle is
`Op0` rather than `Op1`, when the mask indices reach into the "second" half
(indices >= NumElts of the single source), the shuffle reads bytes of `Op0`
instead of `Op1`.

This can produce wrong PSADBW operands in two ways:

1. When `NumSplits > 1`, the splits with `i >= 1` use an iota starting at
   `i*IntrinsicNumElts`. With a two-vector shuffle of `(Op1, Op0)` of equal
   sizes, those indices >= IntrinsicNumElts land in the second operand `Op0`,
   so split `i=1..NumSplits-1` of "Op1" is actually a chunk of `Op0`. PSADBW
   then computes `|op0_chunk - op0_chunk|` for those lanes instead of
   `|op0_chunk - op1_chunk|`.
2. Even when `NumSplits == 1`, the shuffle is morally a self-shuffle of `Op1`,
   but reads bytes from `Op0` for any out-of-range mask entry. With the iota
   mask used here (range `[0, IntrinsicNumElts)`), all entries are in range
   of the first operand for `i=0`, so this single-split case happens to be
   correct. But the multi-split case is broken.

The reduction tree afterwards summates these wrong PSADBW results and the
horizontal-add extract reports them, producing a silent miscompile.

## IR repro sketch

Need NumSplits >= 2: NumElts must be >= 2 * IntrinsicNumElts. On AVX2 with
`Intrinsic::x86_avx2_psad_bw`, `IntrinsicNumElts = 32`, so use NumElts = 64,
which selects AVX-512 `psad_bw_512` (IntrinsicNumElts=64) only if `BWI`.
Without `+avx512bw`, NumElts=64 takes AVX2 path with NumSplits=2 — perfect.

```
; llc -mattr=+avx2 -mtriple=x86_64-- with -mcpu generic, no avx512bw
define i32 @sad_split(<64 x i8> %a, <64 x i8> %b) {
entry:
  %za = zext <64 x i8> %a to <64 x i32>
  %zb = zext <64 x i8> %b to <64 x i32>
  %sub = sub <64 x i32> %za, %zb
  %abs = call <64 x i32> @llvm.abs.v64i32(<64 x i32> %sub, i1 false)
  ; horizontal add reduction tree of <64 x i32> down to i32
  %r = call i32 @llvm.vector.reduce.add.v64i32(<64 x i32> %abs)
  ret i32 %r
}
```

Run through `opt -passes=x86-partial-reduction -mtriple=x86_64-- -mattr=+avx2`.
The transformed IR should contain TWO `@llvm.x86.avx2.psad.bw` calls whose
second arguments are extracts of `%b`. With the bug, the second psadbw call's
second argument is an extract of `%a` (specifically, the upper 32 bytes of
`%a`).

## Expected wrong outcome

Sum-of-absolute-differences computed only on `%a` for half of the lanes,
yielding numeric results unrelated to `%b`. Easy oracle: run the IR with
`lli` (before/after) and compare against a small hand-coded reference, or
compare the post-`x86-partial-reduction` IR against the pre-pass semantics
on a chosen pair of inputs where `%a != %b`.

## Confidence

High — line is a clear typo (compare with line 313 which correctly uses
`(Op0, Op0)`).
