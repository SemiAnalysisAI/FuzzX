# w458 — CoroFrame replaces a non-default-address-space alloca with an `addrspacecast` from the frame's address space, creating UB on AS-aware targets

## Where
`llvm/lib/Transforms/Coroutines/CoroFrame.cpp:983-991`

```c++
  // If the type of Ptr is not equal to the type of AllocaInst, it implies
  // that the AllocaInst may be reused in the Frame slot of other AllocaInst.
  // Note: If the strategy dealing with alignment changes, this cast must be
  // refined
  if (Ptr->getType() != Orig->getType())
    Ptr = Builder.CreateAddrSpaceCast(Ptr, Orig->getType(),
                                      Orig->getName() + Twine(".cast"));
  ...
}
return Ptr;
```

## What

In `createGEPToFramePointer`, when the GEP produced from `Shape.FramePtr` has
a pointer type that does not match the original alloca's pointer type, the
function unconditionally inserts an `addrspacecast` from the frame's address
space to the alloca's address space. With opaque pointers, the only way the
two types can differ is in their **address space** — so this branch generates
an `addrspacecast Ptr to ptr addrspace(N)` to bridge from AS 0 (the typical
coro frame AS) to AS N (the alloca's AS).

This is then used as the alloca's replacement for **every** load/store in
the coroutine, including in the split resume/destroy functions.

## Reproducer

`addrspace.ll`:
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-A5-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr addrspace(5))

define ptr @f() presplitcoroutine {
entry:
  %a = alloca i32, align 4, addrspace(1)
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i32 @llvm.coro.size.i32()
  %alloc = call ptr @malloc(i32 %size)
  %hdl = call noalias ptr @llvm.coro.begin(token %id, ptr %alloc)
  store i32 42, ptr addrspace(1) %a, align 4
  %0 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %0, label %suspend [i8 0, label %use_after
                                i8 1, label %cleanup]
use_after:
  %v = load i32, ptr addrspace(1) %a, align 4
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
opt -passes='cgscc(coro-split),coro-cleanup' -S addrspace.ll
```

Observed:
```
define ptr @f() {
entry:
  %alloc = call ptr @malloc(i32 24)
  ...
  %0 = getelementptr inbounds i8, ptr %alloc, i64 16
  %a.reload.addr = addrspacecast ptr %0 to ptr addrspace(1)
  store i32 42, ptr addrspace(1) %a.reload.addr, align 4
  ...
}
define internal fastcc void @f.resume(ptr noundef nonnull align 8 dereferenceable(24) %hdl) {
entry.resume:
  %0 = getelementptr inbounds i8, ptr %hdl, i64 16
  %a.reload.addr = addrspacecast ptr %0 to ptr addrspace(1)
```

## Why this is a bug

* `addrspacecast` between two unrelated address spaces is a target-defined
  operation. On targets like AMDGPU/NVPTX where AS 0 (generic/global) and
  AS 1 (constant/global) are *not* freely interconvertible (and on AMDGPU
  AS 5 is private/stack), the resulting cast is either a no-op that
  silently mis-points the access (writing to the wrong memory) or a hardware
  trap.
* x86 happens to treat distinct AS numbers as equivalent at the codegen
  level, masking the issue, but the IR-level transform is still incorrect
  for any target where alloca lives in a non-default AS by design.
* The user's `alloca i32, addrspace(1)` expressed a *semantic* requirement
  (this object lives in AS 1). Replacing it with an `addrspacecast` from AS 0
  loses that requirement: the storage in the frame is in AS 0, not AS 1.
* The "Note" in the source comment refers to the alignment-aliasing escape
  hatch only, not to the address-space mismatch this code happily papers
  over.

A correct lowering would either (a) refuse to coroutine-split a function
whose frame contains non-default-AS allocas, or (b) allocate the relevant
slot in the right AS and store a pointer to it in the frame.

## Triggered passes

`cgscc(coro-split)` on a presplit coroutine whose alloca uses an explicit
non-default address space.
