# 262 — MergeICmps merges non-byte-multiple integer compares into a wrong memcmp (miscompile)

Component: `llvm/lib/Transforms/Scalar/MergeICmps.cpp` `visitICmp`.

`visitICmp` records the compare width as `getTypeSizeInBits(opTy)` and the pass
later computes contiguity and the synthesized `memcmp` length in whole bytes
(`SizeBits / 8`). For a type whose bit width is not a byte multiple (e.g. `i17`,
`i1`) the integer division truncates, so the merged `memcmp` compares a
different set of bytes than the original comparison chain and can report a
different result (it also folds in the value's padding bits).

## Miscompile (verified at HEAD via x86 execution)
Two `icmp eq i17` (loads at byte offsets 0 and +2) merge into a byte `memcmp`.
Running the original-semantics IR and the merged output in the same x86_64
binary (Rosetta) disagrees for an input differing only in the affected bits.
`i1` chains degenerate to `memcmp(_,_,0)` (always reports equal).

## Fix
PR [#200346](https://github.com/llvm/llvm-project/pull/200346): bail in
`visitICmp` when the operand bit width is scalable, zero, or not a multiple of 8.
Normal `i8`/`i16`/`i32`/`i64` BCE chains still merge.
