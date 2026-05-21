## Candidate: Red-zone MinSize computation uses uint64_t subtraction that can underflow

File: /home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Target/X86/X86FrameLowering.cpp:1735-1740

### Reasoning
```
uint64_t MinSize =
    X86FI->getCalleeSavedFrameSize() - X86FI->getTCReturnAddrDelta();
```
`getCalleeSavedFrameSize()` returns `unsigned`, but `getTCReturnAddrDelta()` returns
a signed `int` (declared as `int TCReturnAddrDelta = 0;` in X86MachineFunctionInfo).
The standard tail-call usage stores a *negative* delta so that subtracting a negative
yields a larger MinSize (reserving more stack). However the convention is not enforced;
any positive `TCReturnAddrDelta` (e.g. when the callee's stack-args are smaller than
the caller's and the field is set positive) causes the unsigned subtraction
`CalleeSavedFrameSize - TCReturnAddrDelta` to wrap around when
`TCReturnAddrDelta > CalleeSavedFrameSize`, yielding an enormous MinSize. Line 1740
then sets `StackSize = std::max(MinSize, ...)`, which propagates the huge value into
`MFI.setStackSize(StackSize)`. PEI will then attempt to allocate ~2^64 bytes of stack.

Additionally, line 1739 `X86FI->setUsesRedZone(MinSize > 0 || StackSize > 0)` becomes
trivially true (since MinSize is huge), and the red-zone bookkeeping is wrong.

The right computation would be `(int64_t)CalleeSavedFrameSize - (int64_t)TCReturnAddrDelta`
clamped to >= 0 before casting back to uint64_t, or to use `std::max` semantics
explicitly.

### Repro sketch
Craft an MIR test that sets `X86MachineFunctionInfo.TCReturnAddrDelta` to a positive
value (e.g. via `tailcall-storeretaddr` constructs) and has a callee-saved frame size
of 0 with a leaf-like body. The red-zone branch will fire and `setStackSize` will be
called with `(uint64_t)-N`.

### Wrong outcome
Astronomical stack size on a function the compiler thought fit in the red zone, or
ICE later in PEI when the size overflows the frame layout. Even in the "normal"
sign-convention case, this code is fragile because it relies on unspecified
sign-conversion behavior of `unsigned - int`.
