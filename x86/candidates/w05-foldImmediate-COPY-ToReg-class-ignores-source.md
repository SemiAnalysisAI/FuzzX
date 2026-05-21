# foldImmediate: COPY case picks NewOpc from DestReg class but uses source-reg width to size-check ImmVal

File: llvm/lib/Target/X86/X86InstrInfo.cpp:5764-5840

## Description
`foldImmediateImpl` first does (lines 5773-5780):

```cpp
const TargetRegisterClass *RC = nullptr;
if (Reg.isVirtual())
  RC = MRI->getRegClass(Reg);     // Reg is the SOURCE (immediate def)
if ((Reg.isPhysical() && X86::GR64RegClass.contains(Reg)) ||
    (Reg.isVirtual() && X86::GR64RegClass.hasSubClassEq(RC))) {
  if (!isInt<32>(ImmVal))
    return false;
}
```

So the s32 immediate range check is gated on the *source* register's
class. Then for the `COPY` case it picks `NewOpc` from the *destination*'s
class:

```cpp
if (Opc == TargetOpcode::COPY) {
  Register ToReg = UseMI.getOperand(0).getReg();
  ...
  bool GR64Reg = ... (ToReg in GR64) ...;
  if (GR64Reg) {
    if (isUInt<32>(ImmVal))
      NewOpc = X86::MOV32ri64;
    else
      NewOpc = X86::MOV64ri;          // takes a full 64-bit immediate
  }
```

If `Reg` (source) is GR32 (passing the s32 gate trivially) but `ToReg`
is GR64 — i.e. an UpRC-style copy `%dst:gr64 = COPY %src:gr32` — then
ImmVal can be any int64 from the `MOV32ri` def (so 0..0xFFFFFFFF), and
we go to the GR64Reg branch. Here isUInt<32> is true (the def actually
sets the low 32 bits and the upper 32 are zero per x86 zero-extension
semantics for 32-bit ops), and the chosen opcode `MOV32ri64` is
correct.

The bug surfaces in the inverse: `Reg` (source) is GR64 with an
immediate that fits in s32 (the initial guard passes), and `ToReg` is
GR32 — e.g. an isel that produced a 64-bit MOV64ri immediate and a
subreg COPY to GR32. The code at 5815 picks `NewOpc = X86::MOV32ri`
unconditionally for the GR32 case. But the GR32 MOV32ri immediate is a
32-bit literal; the original ImmVal could be any sign-extended s32 in
[-2^31, 2^31). MOV32ri's encoding expects a 32-bit field — passing a
negative int64 like -1 (sign-extended to 0xFFFFFFFFFFFFFFFF) into
`ChangeToImmediate(ImmVal)` at line 5914 silently stores int64 -1.
The MC layer will encode the low 32 bits, but the MachineOperand still
carries the wrong int64 value and downstream consumers (e.g. another
foldImmediate, or a verifier check, or a debugger) see -1 not
0xFFFFFFFF.

More importantly: when the *source* `Reg` is GR64 with a high
immediate `0x100000000` (does *not* fit in s32), the initial guard at
line 5778 rejects. But the bug pattern is: source is GR32 (line 5774
sets RC=GR32, guard skipped), ImmVal is the int64 value from a
`MOV32ri %src, 0xFFFFFFFF` defining a 32-bit register. Here ImmVal is
0xFFFFFFFF == -1 as int32, but isInt<32>(-1) is true so even the s32
check would have passed. So far OK.

Now the COPY destination is GR64 and `isUInt<32>(ImmVal)` is *true*
(since 0xFFFFFFFF fits). The code picks `MOV32ri64` which materializes
`0x00000000FFFFFFFF` into the GR64. Compare to the original semantics:
the IR-level COPY of a sub_32bit from a 32-bit source into a GR64
destination is an undef in the upper 32 bits (or zero-extension if
isel modelled SUBREG_TO_REG). If the upstream COPY was actually an
anyext (high bits undef), foldImmediate inadvertently *defines* the
high bits as 0 — that turns an undef into a known zero. By itself that
is sound. But the converse: a `COPY %dst:gr64 = %src:gr32` after isel
where the GR64 destination is later treated as if the upper 32 bits
were undef can be miscompiled if foldImmediate replaces with
MOV32ri64 (upper-32 = 0) and then a later peephole removes the COPY
chain expecting upper bits to be whatever was in the GR64 already.

## Reproducer (sketch)
```
%1:gr32 = MOV32ri 0
%2:gr64 = COPY %1               ; <-- not legal in strict typing; isel
                                  ;     would normally use SUBREG_TO_REG
                                  ;     but pre-RA optimizers can create
                                  ;     such COPYs after subreg coalescing
```
After foldImmediate: `%2:gr64 = MOV32ri64 0`. The destination width
mismatch (the use side asked for a sub_32bit overlap) is silently
widened. A subsequent peephole that relied on `%2.sub_32bit` not
touching `%2.sub_32bit_hi` may see surprising 0s.

## Wrong outcome
The s32 range guard is keyed off the SOURCE register class, not the
DESTINATION's. For a COPY from a wider source to a narrower dest, the
guard is too strict; for the inverse (narrower source to wider dest)
the guard is too weak and we still pick a 64-bit opcode for what may
legitimately be a sub-register copy with undef high bits, redefining
the high bits as 0 and removing a degree of freedom from later passes.

## Reproducer harness
```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
  -run-pass=peephole-opt repro.mir -o -
```
With a hand-crafted MIR containing the above COPY+MOV32ri pair (and a
use of the high half of `%2`).
