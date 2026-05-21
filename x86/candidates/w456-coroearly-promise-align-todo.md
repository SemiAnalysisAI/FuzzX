# w456 — CoroEarly `lowerCoroPromise` uses a fixed sample-struct offset for the promise, ignoring promise alloca over-alignment

## Where
`llvm/lib/Transforms/Coroutines/CoroEarly.cpp:50-76`

```c++
// Coroutine promise field is always at the fixed offset from the beginning of
// the coroutine frame. i8* coro.promise(i8*, i1 from) intrinsic adds an offset
// to a passed pointer to move from coroutine frame to coroutine promise and
// vice versa. Since we don't know exactly which coroutine frame it is, we build
// a coroutine frame mock up starting with two function pointers, followed by a
// properly aligned coroutine promise field.
// TODO: Handle the case when coroutine promise alloca has align override.
void Lowerer::lowerCoroPromise(CoroPromiseInst *Intrin) {
  Value *Operand = Intrin->getArgOperand(0);
  Align Alignment = Intrin->getAlignment();
  Type *Int8Ty = Builder.getInt8Ty();

  auto *SampleStruct =
      StructType::get(Context, {AnyResumeFnPtrTy, AnyResumeFnPtrTy, Int8Ty});
  const DataLayout &DL = TheModule.getDataLayout();
  int64_t Offset = alignTo(
      DL.getStructLayout(SampleStruct)->getElementOffset(2), Alignment);
  ...
}
```

## What

CoroEarly lowers `@llvm.coro.promise(%hdl, i32 align, i1 from)` at the *caller*
(typically `main`/`destroy`/`resume` callers and similar). It computes the
offset of the promise field in the coroutine frame purely from the `align`
*argument* of the intrinsic, using a synthetic struct
`{ ptr, ptr, i8 }` to model the frame header. The offset is
`alignTo(getElementOffset(i8), Alignment)`.

Meanwhile CoroSplit later builds the actual frame and places the promise at
`alignTo(<size after 2 fn ptrs>, PromiseAlloca->getAlign())` — i.e., using
the *alloca's* alignment (`CoroFrame.cpp:184-191`, `CoroFrame.cpp:818-831`).

If the frontend builds a promise alloca with a stronger alignment than the
intrinsic's `align` argument (or vice versa), the two offsets disagree and
the caller reads the wrong frame slot:

`mismatch.ll`:
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%PromTy = type { i64 }

define ptr @f(i32 %n) presplitcoroutine {
entry:
  %promise = alloca %PromTy, align 32     ; 32-byte alignment
  ...
  %id = call token @llvm.coro.id(i32 0, ptr %promise, ptr null, ptr null)
  ...
}

define i32 @main() {
entry:
  %hdl = call ptr @f(i32 4)
  ; passes align 8 (which "matches" the type but not the alloca)
  %prom = call ptr @llvm.coro.promise(ptr %hdl, i32 8, i1 false)
  %v = load i64, ptr %prom, align 8
  ...
}
```

Observed:
```
; After coro-early on @main:
  %0 = getelementptr inbounds i8, ptr %hdl, i32 16
  %v = load i64, ptr %0, align 8
; After coro-split on @f:
  %promise.reload.addr = getelementptr inbounds i8, ptr %alloc, i64 32
```

So the producer stores at offset 32 while the consumer loads from offset 16:
the load reads adjacent frame bytes (e.g., the spill index field or a
re-loaded `%n.val`) rather than the promise value.

## Why this is interesting

* The TODO at line 56 explicitly acknowledges the missing handling. It is a
  long-standing latent bug.
* While the LangRef states that the `align` argument *should* equal the
  promise alloca's alignment, LLVM does not validate this. A frontend that
  legitimately uses `align <N>` on the alloca to satisfy a runtime
  requirement (cacheline-pinning, SIMD spill area, etc.) but emits
  `@llvm.coro.promise(... i32 <natural alignment of T>, ...)` (the LangRef
  example) silently misreads the promise on the caller side.
* The right fix is to attach the offset (or alignment) to the alloca/Shape so
  CoroEarly can recover it, instead of recomputing from the i32 argument.

## Triggered passes

`coro-early` for the caller side, `cgscc(coro-split)` for the coroutine side.
Default `-O2` enables both when coroutine intrinsics are present.
