## X86AvoidStoreForwardingBlocks: volatile/atomic memcpy load+store split silently

**File:** `llvm/lib/Target/X86/X86AvoidStoreForwardingBlocks.cpp:537-559, 663-728`

### Reasoning

`findPotentiallylBlockedCopies` collects load+store pairs to break apart when a
"blocking" smaller store is detected. The only filters applied are:

- `isPotentialBlockedMemCpyLd(MI.getOpcode())` (XMM/YMM mov pattern)
- `MRI->hasOneNonDBGUse(DefVR)`
- `isPotentialBlockedMemCpyPair(...)`
- `isRelevantAddressingMode(...)`
- `MI.hasOneMemOperand() && StoreMI.hasOneMemOperand()`
- `!alias(LoadMMO, StoreMMO)` (overlap check — only between the two)

`grep -n "isVolatile\|isAtomic\|isUnordered"` on the file returns nothing —
the pass never inspects the `MachineMemOperand`'s volatile/atomic bits. If
the original XMM/YMM load or store is `volatile` (or, on x86, an
`atomic monotonic` 16-byte load/store), `breakBlockedCopies` will split a
single volatile/atomic vector access into 2x XMM, or up to 16x GPR
load/stores. This is a correctness change: a single volatile/atomic memory
operation has been replaced by a sequence of distinct accesses, observable
from another thread or by hardware (MMIO, etc.).

x86 guarantees 16-byte naturally-aligned SSE load/store atomicity on
suitable hardware; splitting destroys that guarantee. Volatile splitting is
forbidden by the C/C++ memory model.

### MIR sketch

```mir
; A 16-byte volatile copy that survived as a single XMM load/store pair,
; with a small "blocking" volatile/non-volatile MOV32mi nearby:
%vr0:vr128 = VMOVUPSrm $rdi, 1, $noreg, 0, $noreg ::
    (volatile load (s128) from %ir.src)
MOV32mi $rsp, 1, $noreg, 4, $noreg, 0 ::
    (store (s32) into %stack.0)        ; "blocker"
VMOVUPSmr $rsi, 1, $noreg, 0, $noreg, %vr0 ::
    (volatile store (s128) into %ir.dst)
```

After pass: the volatile load and store are replaced by 2x MOV64rm + 2x
MOV64mr (or similar), each carrying a clone of the volatile MMO. Even if
the volatile bit is propagated by `getMachineMemOperand(MMO, Off, Size)`,
the user-visible number of accesses has doubled, which is observable for
volatile and breaks `atomic` width guarantees.

### What's wrong

The pass needs to bail when
`(*MI.memoperands_begin())->isVolatile() || isAtomic()` (and likewise for
the store) before adding to `BlockedLoadsStoresPairs`.

### Concrete fix sketch

In `findPotentiallylBlockedCopies`, in the inner `if` (line ~549), add:

```cpp
const MachineMemOperand *LMMO = *MI.memoperands_begin();
const MachineMemOperand *SMMO = *StoreMI.memoperands_begin();
if (LMMO->isVolatile() || LMMO->isAtomic() ||
    SMMO->isVolatile() || SMMO->isAtomic())
  continue;
```
