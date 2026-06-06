# 024 - `store undef` deletion can expose a prior poison store

Component: `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:1575`

InstCombine currently treats `store undef, Ptr` as a no-op:

```text
store undef, Ptr -> noop
```

The adjacent FIXME notes that this is technically incorrect if the store
overwrites poison. This reproducer stores poison to memory, stores `undef` over
it, and then reloads. In the source, the `undef` store may choose any concrete
non-poison value for the memory slot. After InstCombine deletes the `undef`
store, the reload reads the earlier poison.

This manifests with ordinary integral pointers and does not depend on
non-integral pointer representation details. Sibling folds such as deleting
`memset` of undef have the same root cause and should not be counted as a
separate unique bug.

Verifier: Kuhn (019e9920-6c4d-7870-bb16-082306e115e6) returned YES.
