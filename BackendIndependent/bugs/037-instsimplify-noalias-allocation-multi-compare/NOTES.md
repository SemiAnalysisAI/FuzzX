# 037 - non-escaping allocation compare folded independently for all addresses

Component: `llvm/lib/Analysis/InstructionSimplify.cpp:2830`

`computePointerICmp` has a FIXME for PR54002: InstSimplify can fold a compare
between a non-escaping allocation and a loaded non-null pointer to false by
assuming the allocation address can be chosen to make the comparison false. That
does not compose across multiple comparisons to the same allocation.

This reproducer uses a 4-bit integral pointer datalayout, so there are 15
non-null address values. The volatile `!nonnull` global loads can cover every
non-null address. A `nonnull` allocation must therefore equal one of them, so
the source can return true. InstCombine currently folds every comparison
independently and returns `false`.

This is an ordinary integer-pointer target issue. It does not depend on
non-integral pointer external state or non-address bits.

Verifier: Maxwell (019e9959-a0eb-7823-8f44-350ebc6f2965) returned YES.
