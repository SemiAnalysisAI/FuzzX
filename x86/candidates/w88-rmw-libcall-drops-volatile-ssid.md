file: llvm/lib/CodeGen/AtomicExpandPass.cpp:1996-2005, 2043+

`expandAtomicRMWToLibcall` (and similarly
`expandAtomicLoadToLibcall`, `expandAtomicStoreToLibcall`,
`expandAtomicCASToLibcall`) calls `expandAtomicOpToLibcall` with
only:
  - size, alignment, pointer, value(/compare), success ordering, failure ordering
The libcall signature (`__atomic_fetch_*`, `__atomic_load`,
`__atomic_store`, `__atomic_compare_exchange*`) carries:
  - pointer, value(s), i32 ordering (and ordering2 for CAS)
It does NOT carry `volatile` or syncscope.

For x86_64 this fires on `atomicrmw` types that exceed the native
width (i128, libcall fallback for fmax/fmin/uincwrap/etc when the
size-specialized libcall is unavailable and the CAS-loop libcall
is used).

Reproducer:

  define i128 @nand_vol_i128(ptr %p, i128 %v) {
    %x = atomicrmw volatile nand ptr %p, i128 %v syncscope("singlethread") seq_cst, align 16
    ret i128 %x
  }

`opt -mtriple=x86_64-unknown-linux-gnu -atomic-expand -S` output:

  define i128 @nand_vol_i128(ptr %p, i128 %v) {
    %1 = call i128 @__atomic_fetch_nand_16(ptr %p, i128 %v, i32 5)
    ret i128 %1
  }

  declare i128 @__atomic_fetch_nand_16(ptr, i128, i32)

Two issues:

1. `volatile` is silently dropped. The libcall may legally elide
   the access (e.g., if the runtime decides the address is
   thread-local). For MMIO-style code the user wrote `volatile`
   precisely so the access is performed exactly once; the libcall
   makes no such guarantee.

2. `syncscope("singlethread")` is silently widened to
   cross-thread. This is "stronger" so likely benign for the C++
   memory model, but it defeats the user-requested optimization
   (a singlethread RMW is supposed to compile to a non-locked
   sequence). The libcall always issues a lock-prefixed sequence
   on x86, increasing latency by ~10x for the
   intended-single-thread use case.

   Worse, if `syncscope` is a custom address-space-restricted
   scope (used by GPU-style backends linked into x86 via lib),
   silently widening it to system scope is a real correctness
   regression — code that expected scoped atomics can deadlock
   if the runtime libcall's lock backs onto a contended global
   lock.

Fix options:
  - When `volatile`, refuse to lower to a libcall (emit a
    diagnostic, fall back to inline CAS expansion).
  - When `SSID != System`, refuse to lower (or emit the
    diagnostic) — there is no libcall ABI for scoped atomics.
  - At minimum, attach the volatile bit / syncscope onto the call
    via `MOVolatile` / metadata so the backend can recover.

Same pattern in `expandAtomicCASToLibcall` for weak/volatile/ssid
cmpxchg and in `expandAtomicLoadToLibcall`/`...StoreToLibcall`.
