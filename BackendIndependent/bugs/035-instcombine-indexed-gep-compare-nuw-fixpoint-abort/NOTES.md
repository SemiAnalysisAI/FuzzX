# 035 - indexed GEP compare transform exposes later `nuw` inference

Component: `llvm/test/Transforms/InstCombine/indexed-gep-compares.ll`

`foldGEPICmp` / `transformToIndexedCompare` rewrites a pointer compare into an
offset compare. After compare canonicalization proves the offset nonnegative,
a second InstCombine iteration can add `nuw` to the exit GEP. The current pass
does not reach the same result in one verified iteration.

The related `opaque-ptr.ll@indexed_compare` test has the same root cause and is
not counted separately.

Verifier: Ptolemy (019e9951-3a70-74b0-a514-cb7cea990af0) returned YES.
