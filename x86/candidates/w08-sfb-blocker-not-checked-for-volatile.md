## X86AvoidStoreForwardingBlocks: "blocking store" allowed to be volatile/atomic

**File:** `llvm/lib/Target/X86/X86AvoidStoreForwardingBlocks.cpp:685-701`

### Reasoning

After finding the candidate memcpy load+store, the pass walks instructions
backward via `findPotentialBlockers` and asks whether each one is a
blocking store via `isPotentialBlockingStoreInst`. The filtering is:

```cpp
if (!isPotentialBlockingStoreInst(PBInst->getOpcode(),
                                  LoadInst->getOpcode()) ||
    !isRelevantAddressingMode(PBInst) || !PBInst->hasOneMemOperand())
  continue;
```

There is no check on the blocker's MMO for `volatile`/`atomic`. If the
intervening "blocker" is a volatile store (e.g., a MMIO write that the
user inserted between two ordinary writes), the pass treats it as a normal
blocker and splits the load+store around it.

The downstream effect: the partial copies introduced by the split are
inserted *adjacent to* the (still-emitted) volatile blocker, which is
fine in isolation, but the original semantic intent — "perform one
volatile MMIO followed by one big copy" — is broken into "many small
copies + the volatile MMIO," changing the number and ordering of memory
accesses the user can observe via volatile semantics, since some of the
new partial loads/stores can be scheduled around the volatile one.

This is less severe than the load/store side (those become **non**
atomic/volatile in the rewrite) but still a soundness concern: the
pass should bail out when an aliasing volatile/atomic memory op sits
between the load and store.

### What's wrong

`isPotentialBlockingStoreInst` filter is too permissive — does not look at
`MachineMemOperand` flags. Combined with the load/store volatile bug
(separate candidate file), this gives the pass three places where
volatile semantics are silently violated.

### Suggested fix

```cpp
auto *PBMMO = *PBInst->memoperands_begin();
if (PBMMO->isVolatile() || PBMMO->isAtomic()) {
  // Conservatively give up on this entire pair.
  BlockingStoresDispSizeMap.clear();
  break;
}
```
