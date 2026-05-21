file: llvm/lib/Target/X86/X86ISelLowering.cpp:33007-33058
(also leaning on llvm/lib/CodeGen/AtomicExpandPass.cpp:1701-1734)

X86TargetLowering::lowerIdempotentRMWIntoFencedLoad rewrites an
idempotent `atomicrmw` (e.g. `atomicrmw add %p 0`) into:

  fence seq_cst <ssid>
  %v = load atomic ...      ; CreateAlignedLoad(...)
  ... setAtomic(Order, SSID)

It propagates the type, alignment, ordering, syncscope and pcsections
metadata of the original atomicrmw. It does NOT copy the volatile flag.
`isIdempotentRMW` in AtomicExpandPass also does not exclude volatile
atomicrmws. The combination silently strips `volatile` from a
`volatile atomicrmw or %p 0` (and similar idempotent variants).

For volatile atomics, IR semantics require the access to be performed
exactly as written (same width, same number of accesses, never elided
or replaced). Substituting a non-volatile load is a wrong-code bug for
MMIO-style code that uses idempotent volatile RMWs.

Candidate IR sketch:

  define i32 @f(ptr %p) {
    %x = atomicrmw volatile or ptr %p, i32 0 seq_cst, align 4
    ret i32 %x
  }

Expected: a locked instruction (e.g. `lock or` or a cmpxchg loop)
that respects volatility.

Observed (likely): a plain `mov %p, %eax` after an mfence, with the
volatile bit dropped.

Fix: in lowerIdempotentRMWIntoFencedLoad, bail out (return nullptr) if
`AI->isVolatile()`, OR call `Loaded->setVolatile(AI->isVolatile())`
after the CreateAlignedLoad.
