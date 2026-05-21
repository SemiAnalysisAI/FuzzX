# 001 — `atomicrmw volatile or i32 0` strips the volatile bit on lowering

Component: X86ISelLowering (also AtomicExpandPass)

## Source

`llvm/lib/Target/X86/X86ISelLowering.cpp` ::
`X86TargetLowering::lowerIdempotentRMWIntoFencedLoad` (around lines 33007–33058).

```cpp
// Replace the rmw with a fence + load.
auto *Loaded = Builder.CreateAlignedLoad(VT, AI->getPointerOperand(),
                                         AI->getAlign());
// ... copies type/align/order/syncscope/pcsections ...
//     but NOT AI->isVolatile()
```

`AtomicExpandPass::isIdempotentRMW` (around `llvm/lib/CodeGen/AtomicExpandPass.cpp:1701-1734`)
also does not exclude volatile RMWs.

## Symptom

For the volatile RMW:

```ll
%x = atomicrmw volatile or ptr %p, i32 0 seq_cst, align 4
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu` emits:

```
lock orl  $0, -64(%rsp)   ; mfence emulation via locked stack-or
movl  (%rdi), %eax        ; PLAIN (non-volatile) load
```

The output is bit-identical to the non-volatile version, which is the smoking gun.

## Why this is a wrong-code bug

LangRef on volatile (https://llvm.org/docs/LangRef.html#volatile-memory-accesses):

> An LLVM volatile operation [...] must not be changed in any way other than
> changing its default ordering and atomicity. The number of volatile operations
> on any particular memory location [...] must not be changed.

The RMW is replaced by a non-volatile load (different operation, different
semantics) — that violates the spec. A subsequent pass (GVN, instcombine,
DAGCombiner) is free to fold or eliminate the non-volatile load, which would
elide the user's volatile access entirely. This matters for MMIO and device
memory.

## Fix sketches

In `X86TargetLowering::lowerIdempotentRMWIntoFencedLoad`:

```cpp
if (AI->isVolatile()) return nullptr;
```

or copy the volatile bit:

```cpp
Loaded->setVolatile(AI->isVolatile());
```

The same guard is needed in `AtomicExpandPass::isIdempotentRMW`.

## Reproducer

`repro.ll` + `cmd.sh`. Run `./cmd.sh`; the two functions emit identical asm.
