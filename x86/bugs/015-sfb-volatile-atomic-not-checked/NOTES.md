# 015 — X86AvoidStoreForwardingBlocks ignores volatile/atomic MMOs

Component: X86AvoidStoreForwardingBlocks (default x86 -O>0 pipeline)

## Source

`llvm/lib/Target/X86/X86AvoidStoreForwardingBlocks.cpp:537-559, 663-728`

`findPotentiallylBlockedCopies` collects load+store pairs to break apart
when a smaller "blocking" store is detected. The filters consulted are:

- `isPotentialBlockedMemCpyLd(MI.getOpcode())` (XMM/YMM mov pattern)
- `MRI->hasOneNonDBGUse(DefVR)`
- `isPotentialBlockedMemCpyPair(...)`
- `isRelevantAddressingMode(...)`
- `MI.hasOneMemOperand() && StoreMI.hasOneMemOperand()`
- `!alias(LoadMMO, StoreMMO)`

`grep -n 'isVolatile\|isAtomic\|isUnordered' X86AvoidStoreForwardingBlocks.cpp`
returns **nothing** — the pass never inspects the MMO's volatile or atomic
bit. `breakBlockedCopies` then splits the user's 16/32-byte volatile/atomic
load+store into 2× XMM or up to 16× GPR pairs.

LangRef requires volatile memory operations to be preserved exactly (number,
size, ordering). On x86, naturally-aligned 16-byte SSE accesses are
hardware-atomic; splitting destroys that guarantee.

## Fix sketch

In `findPotentiallylBlockedCopies` add (≈ line 549):

```cpp
const MachineMemOperand *LMMO = *MI.memoperands_begin();
const MachineMemOperand *SMMO = *StoreMI.memoperands_begin();
if (LMMO->isVolatile() || LMMO->isAtomic() ||
    SMMO->isVolatile() || SMMO->isAtomic())
  continue;
```

Source-confirmed via `grep`. The triggering MIR shape is narrow but
documented in the candidate file `../candidates/w08-sfb-volatile-atomic-not-checked.md`.

## Repro sketch

```mir
; volatile 16-byte XMM copy with a small "blocker" store
%vr0:vr128 = VMOVUPSrm $rdi, 1, $noreg, 0, $noreg ::
    (volatile load (s128) from %ir.src)
MOV32mi $rsp, 1, $noreg, 4, $noreg, 0 ::
    (store (s32) into %stack.0)
VMOVUPSmr $rsi, 1, $noreg, 0, $noreg, %vr0 ::
    (volatile store (s128) into %ir.dst)
```
