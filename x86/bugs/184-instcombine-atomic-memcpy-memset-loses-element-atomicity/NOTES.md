file: llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:151-218 (memcpy)
      llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:264-285 (memset)

`SimplifyAnyMemTransfer` and `SimplifyAnyMemSet` fold short-length
`llvm.memcpy.element.unordered.atomic.*` and
`llvm.memset.element.unordered.atomic.*` into a single load+store /
single store using an i{Len*8} type, e.g.

  memcpy(.atomic, align=4, elt=1, len=4)
    --> %v = load atomic i32 unordered, store atomic i32 %v unordered

The element-size argument (last `i32 immarg`) — the entire reason this
intrinsic exists, distinct from plain memcpy — is silently discarded.
The check that gates the transform only looks at length vs. alignment:

  // line 170-172
  if (MI->isAtomic())
    if (*CopyDstAlign < Size || *CopySrcAlign < Size)
      return nullptr;

It does not check `MI->getElementSizeInBytes()`. The same omission
appears at lines 261-262 for memset.

LangRef on these intrinsics is explicit that each `element_size`-sized
chunk is independently atomic and unordered with respect to other
operations on the same memory; the order between chunks is unspecified.
Collapsing four 1-byte unordered-atomic writes into a single 4-byte
unordered-atomic write changes the observable atomicity granularity.

Concrete miscompile setup (opt -passes=instcombine):

  ; INPUT
  declare void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(
      ptr nocapture writeonly, ptr nocapture readonly,
      i64, i32 immarg)
  define void @f(ptr %d, ptr %s) {
    call void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(
        ptr align 4 %d, ptr align 4 %s, i64 4, i32 1)
    ret void
  }

  ; OUTPUT
  define void @f(ptr %d, ptr %s) {
    %1 = load atomic i32, ptr %s unordered, align 4
    store atomic i32 %1, ptr %d unordered, align 4
    ret void
  }

Concurrent reader scenario:

  Thread A:  memcpy.element.atomic %dst, %src, len=4, elt=1   ; bytes of 0xAA
  Thread B:  %v = load atomic i32, ptr %dst unordered

Under original semantics, B may observe ANY of:
  0x00000000, 0xAA000000, 0xAAAA0000, 0xAAAAAA00, 0xAAAAAAAA
(per LangRef "order of the writes is unspecified", but each byte is its
own atomic event).

After the transform, B only observes 0x00000000 or 0xAAAAAAAA.

The transform is also reachable for memset
(test: `memset.element.atomic elt=1 size=4` -> single i32 store) — same
issue, verified.

Fix: in the `MI->isAtomic()` guard, additionally require
`MI->getElementSizeInBytes() >= Size` (i.e., the intrinsic already
treats the whole block as one element). When that fails, leave the
intrinsic alone or expand into N element-sized atomic load/stores.

Severity: low-to-medium. Affects code using
`llvm.{memcpy,memset}.element.unordered.atomic` (Java GC, lock-free
buffers). It strengthens atomicity (fewer observable interleavings),
so most programs are unaffected, but a program that *uses* the weaker
guarantee — e.g. a lock-free writer signaling progress via partial
fills observable to a reader — is silently broken.
