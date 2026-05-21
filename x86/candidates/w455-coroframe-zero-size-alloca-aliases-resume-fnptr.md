# w455 — CoroFrame zero-size alloca is mapped to frame field 0 (the resume function pointer slot)

## Where
`llvm/lib/Transforms/Coroutines/CoroFrame.cpp:246-249`

```c++
[[nodiscard]] FieldIDType addField(uint64_t FieldSize, Align FieldAlignment,
                                   bool IsHeader = false) {
  assert(!IsFinished && "adding fields to a finished builder");

  // For an alloca with size=0, we don't need to add a field and they
  // can just point to any index in the frame. Use index 0.
  if (FieldSize == 0)
    return 0;
```

## What

`FrameTypeBuilder::addField` returns `FieldIDType 0` for any zero-size field.
The corresponding `FrameData.setFieldIndex(alloca, 0)` maps the alloca's offset
to field 0 in the frame. For the Switch ABI, field 0 is the resume function
pointer (see `CoroFrame.cpp:823`, where the first header field is
`FnPtrTy`/`AnyResumeFnPtrTy`). The reload GEP therefore becomes
`getelementptr inbounds i8, ptr %hdl, i64 0`, which points at the live resume
function-pointer slot rather than to a unique zero-sized location.

LLVM's IR semantics treat an `alloca` (including a zero-size one such as
`alloca [0 x i8]`) as a unique address that does not alias other objects. Any
later access through the reload pointer (even bytes that the user later writes
past the nominal zero size, or a comparison-by-pointer-identity) now silently
aliases the frame's resume function pointer. A subsequent `store` through the
returned pointer would overwrite the resume pointer and corrupt the coroutine
state machine.

## Reproducer

`zero_alloca.ll`:
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr)

define ptr @f() presplitcoroutine {
entry:
  %zero = alloca [0 x i8]
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i32 @llvm.coro.size.i32()
  %alloc = call ptr @malloc(i32 %size)
  %hdl = call noalias ptr @llvm.coro.begin(token %id, ptr %alloc)
  %0 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %0, label %suspend [i8 0, label %use_zero
                                i8 1, label %cleanup]
use_zero:
  call void @use(ptr %zero)
  ret ptr %hdl
cleanup:
  br label %suspend
suspend:
  call void @llvm.coro.end(ptr %hdl, i1 false, token none)
  ret ptr %hdl
}

declare ptr @malloc(i32)
declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i32 @llvm.coro.size.i32()
declare ptr @llvm.coro.begin(token, ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare void @llvm.coro.end(ptr, i1, token)
```

Run:
```
opt -passes='cgscc(coro-split),coro-cleanup' -S zero_alloca.ll
```

Observed:
```
define internal fastcc void @f.resume(ptr noundef nonnull align 8 dereferenceable(24) %hdl) {
entry.resume:
  %zero.reload.addr = getelementptr inbounds i8, ptr %hdl, i64 0
  ...
use_zero:
  call void @use(ptr %zero.reload.addr)
```

`%zero.reload.addr` is `%hdl + 0`, i.e. the resume function pointer field.
The IR hands the resume-pointer address out to user code under the guise of
the user's `%zero` alloca.

## Why it is a bug

* Breaks the uniqueness guarantee of `alloca`: two different zero-size allocas
  in the same coroutine are mapped to the same address (offset 0), and also
  collide with the resume function pointer.
* Any subsequent write through the reload pointer (a memset, memcpy with a
  zero length argument that the optimizer later proves non-zero, a
  user-extended access, or even an opaque escape that the optimizer assumes
  cannot alias the frame header) can overwrite the resume function pointer
  with arbitrary data, breaking the coroutine.
* The shortcut is documented in the comment ("can just point to any index in
  the frame. Use index 0") but using "index 0" inside the Switch frame is a
  poor choice because index 0 is a live, dereferenced header field.

A correct fix would either reserve a dedicated zero-size scratch field or
have the reload GEP point to a location past the end of the live frame
(e.g. `Shape.FrameSize`) so that aliasing the live header is impossible.

## Second-order observable

Multiple zero-size allocas collapse to *the same* GEP. The reproducer below
makes the alias observable through `icmp eq` and writes the result back at
offset 0, corrupting the resume function pointer:

```llvm
define ptr @f() presplitcoroutine {
entry:
  %zero1 = alloca [0 x i8]
  %zero2 = alloca [0 x i8]
  %eq = icmp eq ptr %zero1, %zero2   ; semantically false (or implementation-defined)
  ...
use_after:
  store i1 %eq, ptr %zero1           ; writes one byte at frame offset 0
  ret ptr %hdl
}
```

Output of `f.resume`:
```
%zero1.reload.addr = getelementptr inbounds i8, ptr %hdl, i64 0
%zero2.reload.addr = getelementptr inbounds i8, ptr %hdl, i64 0
...
%eq1 = icmp eq ptr %zero1.reload.addr, %zero2.reload.addr   ; trivially true
store i1 %eq1, ptr %zero1.reload.addr, align 1              ; clobbers fn ptr LSB
```

`%eq1` is now a literal `i1 true` (same GEP), and the `store` clobbers the
low byte of `@f.resume`'s pointer in the frame, breaking subsequent
`@llvm.coro.resume`/`@llvm.coro.destroy` calls.

## Triggered passes

`coro-early,coro-split,coro-cleanup` (Switch ABI). The `presplitcoroutine`
attribute is needed; nothing else target-specific is required.
