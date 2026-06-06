# 038 - scalarizePHI inserts after an invoke terminator

Component: `llvm/lib/Transforms/InstCombine/InstCombineVectorOps.cpp:171`

`scalarizePHI` creates an `extractelement` for each incoming PHI value. When an
incoming value is an instruction, it inserts after that instruction:

```text
InsertPos = ++pos->getIterator()
```

If the incoming value is an `invoke`, the value is defined on the normal edge to
the successor and is a valid PHI incoming value. But the `invoke` is also the
terminator of its block, so incrementing its iterator reaches the block
sentinel and assertion-enabled InstCombine crashes.

Verifier: Kant the 2nd (019e9963-871f-75b2-884b-cf8e26defeaa) returned YES.
