## X86AvoidStoreForwardingBlocks: trailing buildCopies passes LMMOffset where SMMOffset is expected

**File:** `llvm/lib/Target/X86/X86AvoidStoreForwardingBlocks.cpp:607-609`

### Reasoning

In `breakBlockedCopies`, after the loop over the blocking-stores map, the
final tail copy is emitted:

```cpp
unsigned Size3 = (LdDispImm + getRegSizeInBytes(LoadInst)) - LdDisp1;
buildCopies(Size3, LoadInst, LdDisp1, StoreInst, StDisp1, LMMOffset,
            LMMOffset);
```

The 6th and 7th parameters of `buildCopies` are `LMMOffset` and `SMMOffset`
respectively, and they are subsequently used to construct **two distinct**
`MachineMemOperand`s: one for the new load (via `LMMO` at line 407) and one
for the new store (via `SMMO` at line 427). Passing `LMMOffset` for both
means the new tail-store's MMO is offset against the store base by the
**load**'s accumulated offset.

In current code, `LMMOffset` and `SMMOffset` happen to stay in lockstep
(both incremented by the same `Size1 + Size2` amounts at lines 604-605 and
by the same `Size` amounts inside `buildCopies` at lines 450-451), so the
two values are always equal at this point. That makes the bug **latent**
rather than user-visible today, but:

1. It is fragile. Any future change that lets these offsets diverge (e.g.,
   handling load/store with different element widths, or starting offsets)
   silently produces wrong MMO offsets — wrong aliasing info fed to later
   passes (post-RA scheduler, MachineCSE, etc.), which can cause miscompiles.
2. The intent is clearly to pass `SMMOffset` (matches the in-loop calls at
   lines 597-598 and 600-601 which pass `LMMOffset, SMMOffset` and
   `LMMOffset + Size1, SMMOffset + Size1`).

### MIR sketch

Trigger by a memcpy load+store with a blocking store whose displacement
makes only a partial overlap, so that after the loop the tail `Size3 > 0`.
Today the bug is invisible because LMMOffset==SMMOffset.

### What's wrong

Passing `LMMOffset` twice instead of `LMMOffset, SMMOffset` — likely a
typo. Fix to `SMMOffset`.

### Suggested fix

```cpp
buildCopies(Size3, LoadInst, LdDisp1, StoreInst, StDisp1, LMMOffset,
            SMMOffset);
```
