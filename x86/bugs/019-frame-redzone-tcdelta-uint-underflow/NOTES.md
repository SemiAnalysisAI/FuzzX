# 019 — `X86FrameLowering` red-zone `MinSize` underflows on positive `TCReturnAddrDelta`

Component: X86FrameLowering

## Source

`llvm/lib/Target/X86/X86FrameLowering.cpp:1735-1740`

```cpp
uint64_t MinSize =
    X86FI->getCalleeSavedFrameSize() - X86FI->getTCReturnAddrDelta();
```

`getCalleeSavedFrameSize()` returns `unsigned`, but `getTCReturnAddrDelta()`
returns a signed `int` (`int TCReturnAddrDelta = 0;` in
X86MachineFunctionInfo). The standard tail-call convention stores a
**negative** delta, so subtracting a negative ends up enlarging MinSize
appropriately. The convention is *not enforced*, however; any positive
`TCReturnAddrDelta > CalleeSavedFrameSize` makes the unsigned subtraction
wrap around to a huge value. Line 1740 then `setStackSize(MinSize)` and
PEI later tries to allocate the absurd stack.

Even in the normal sign-convention case, the code is fragile because it
relies on unspecified `unsigned - int` conversion behavior.

## Fix

```cpp
int64_t MinSizeSigned = (int64_t)X86FI->getCalleeSavedFrameSize()
                       - (int64_t)X86FI->getTCReturnAddrDelta();
uint64_t MinSize = (MinSizeSigned > 0) ? (uint64_t)MinSizeSigned : 0;
```

## Reproducer

Requires MIR — the IR layer has no way to set `TCReturnAddrDelta` to a
positive value directly, but a synthesized MIR test with
`tailcall-storeretaddr` constructs reaches the same code path.

Source-confirmed.
