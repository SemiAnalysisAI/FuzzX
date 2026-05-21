# w457 — CoroSplit retcon/retcon.once `emitAlloc` does not communicate the required frame alignment to the user-provided allocator

## Where
`llvm/lib/Transforms/Coroutines/CoroSplit.cpp:1827-1846`
`llvm/lib/Transforms/Coroutines/Coroutines.cpp:497-518`
`llvm/lib/Transforms/Coroutines/SpillUtils.cpp:67-92`

```c++
// CoroSplit.cpp
if (Shape.RetconLowering.IsFrameInlineInStorage) {
  RawFramePtr = Id->getStorage();
} else {
  IRBuilder<> Builder(Id);

  auto FrameSize = Builder.getInt64(Shape.FrameSize);

  // Allocate.  We don't need to update the call graph node because we're
  // going to recompute it from scratch after splitting.
  // FIXME: pass the required alignment
  RawFramePtr = Shape.emitAlloc(Builder, FrameSize, nullptr);
  ...
}
```

```c++
// Coroutines.cpp
Value *coro::Shape::emitAlloc(IRBuilder<> &Builder, Value *Size,
                              CallGraph *CG) const {
  switch (ABI) {
  case coro::ABI::Switch:
    llvm_unreachable("can't allocate memory in coro switch-lowering");

  case coro::ABI::Retcon:
  case coro::ABI::RetconOnce: {
    auto Alloc = RetconLowering.Alloc;
    Size = Builder.CreateIntCast(Size,
                                 Alloc->getFunctionType()->getParamType(0),
                                 /*is signed*/ false);
    auto *Call = Builder.CreateCall(Alloc, Size);
    ...
  }
```

## What

In Retcon / RetconOnce ABI, when the frame does not fit inline in the
continuation storage, CoroSplit allocates the frame by calling the
user-supplied `alloc` function, passing only the requested **size**. The
`alloc` function's signature is `i8* (iN)` — there is no parameter for the
required alignment, and CoroSplit never tags the resulting pointer with an
`align` attribute either.

`Shape.FrameAlign` is known at this point (it was just computed by
`buildFrameLayout`, `CoroFrame.cpp:876`), and it can be strictly larger than
the natural alignment of any allocator (e.g., if the frame contains a
`<8 x double>` value with ABI align 64, or a high-aligned alloca). The
allocator has no way to honor that alignment, so every subsequent load/store
that the lowering emits with `align Shape.FrameAlign` may execute against a
pointer whose actual run-time alignment is lower than what LLVM's
optimizer/codegen assumes.

The companion path `SpillUtils.cpp:lowerNonLocalAlloca` has the identical
issue:

```c++
auto Alloc = Shape.emitAlloc(Builder, AI->getSize(), nullptr);
```

It allocates a dynamic frame for a coro.alloca.alloc with the user's
declared alignment information thrown away (`AI->getAlign()` is never
passed).

## Why this is interesting

* The FIXME at `CoroSplit.cpp:1839` acknowledges the missing alignment.
* Most C++ coroutine ABIs use Switch lowering, but the Retcon ABI is used
  for Swift async functions and other front ends, where over-alignment of
  the frame (or of spilled values) silently invokes UB at run time.
* Even on x86, an SSE/AVX value in the frame with ABI align 16/32 paired
  with a malloc-style allocator that only guarantees 8-byte alignment would
  produce misaligned `movdqa`/`vmovaps` in the resume function.
* The fix requires changing the user-visible signature contract of the
  retcon `alloc` function (or adding an alignment shim that
  over-allocates and aligns up).

## Triggered passes

`cgscc(coro-split)` on a Retcon or RetconOnce coroutine with a spilled value
or alloca whose ABI alignment exceeds the allocator's default alignment.
