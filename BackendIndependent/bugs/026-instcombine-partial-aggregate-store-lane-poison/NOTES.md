# 026 - Partial aggregate store rewritten to whole-vector store propagates poison

Component: `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:1235`

`likeBitCastFromVector()` recognizes a value that looks like an aggregate built
from vector extracts, and `combineStoreToValueType()` rewrites the aggregate
store as a store of the original vector type.

In this reproducer only element 0 of the aggregate is populated from `%u`;
element 1 remains `undef`. InstCombine rewrites the store of `[2 x i8]` to:

```llvm
store <2 x i8> %u, ptr %p, align 1
```

For `%u = <0, poison>`, the source stores an `undef` byte to lane 1, which may
be chosen as a concrete non-poison byte. The optimized program stores poison to
lane 1, so the following load can become poison. This is a value miscompile on
ordinary integer-pointer targets.

Struct-shaped and load-forwarding variants have the same root cause and should
not be counted separately.

Verifier: Zeno (019e9930-e156-7042-a7f1-2d2a5dbe15ec) returned YES.
