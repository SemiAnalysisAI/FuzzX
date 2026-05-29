# 186 — LICM `promoteLoopAccessesToScalars` sinks N conditional stores into single exit-store via "thread-local" gate

Component: LICM

LICM's `isThreadLocalObject` treats all `alloca`s as thread-local for the purpose of "stores are safe to insert into the exit block." That ignores in-thread observers (async signal handler, setjmp landingpad, `__attribute__((cleanup))`) that may read the alloca via a captured side-channel pointer (e.g. captured via inline asm or `escape` intrinsic that defeats SSA-flow capture analysis).

When the loop performs N conditional stores to the alloca, LICM can promote to a single sunk exit-store of the final loop value. If the alloca's address escaped via a non-SSA channel and a signal handler reads it mid-loop, the handler observes a stale value (or no intermediate write at all).

Sibling of #126 / #144 / #160 / #161 / #185 — different aspect of the same `promoteLoopAccessesToScalars` defect cluster.

Source: `llvm/lib/Transforms/Scalar/LICM.cpp` (`promoteLoopAccessesToScalars`, `isThreadLocalObject`).

Demo: file an IR that escapes an alloca via inline-asm sideeffect and stores to it conditionally in a loop, then verify `opt -passes='loop-mssa(licm)'` sinks the store outside the loop.
