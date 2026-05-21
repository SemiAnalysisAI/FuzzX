# reMaterialize replaces MOV32r0/r1/r_1 with MOV32ri but ignores SubIdx for the def

File: llvm/lib/Target/X86/X86InstrInfo.cpp:961-997

## Description

```cpp
void X86InstrInfo::reMaterialize(MachineBasicBlock &MBB,
                                 MachineBasicBlock::iterator I,
                                 Register DestReg, unsigned SubIdx,
                                 const MachineInstr &Orig,
                                 LaneBitmask UsedLanes) const {
  bool ClobbersEFLAGS = Orig.modifiesRegister(X86::EFLAGS, &TRI);
  if (ClobbersEFLAGS && MBB.computeRegisterLiveness(&TRI, X86::EFLAGS, I) !=
                            MachineBasicBlock::LQR_Dead) {
    // Re-materialize as MOV32ri to avoid side effects.
    int Value;
    switch (Orig.getOpcode()) { ... }

    const DebugLoc &DL = Orig.getDebugLoc();
    BuildMI(MBB, I, DL, get(X86::MOV32ri))
        .add(Orig.getOperand(0))
        .addImm(Value);
  } else {
    MachineInstr *MI = MBB.getParent()->CloneMachineInstr(&Orig);
    MBB.insert(I, MI);
  }

  MachineInstr &NewMI = *std::prev(I);
  NewMI.substituteRegister(Orig.getOperand(0).getReg(), DestReg, SubIdx, TRI);
}
```

The MOV32r0/MOV32r1/MOV32r_1 pseudos are typed `(outs GR32:$dst)`. The
rewrite to `MOV32ri` keeps the GR32 def width. If the caller of
`reMaterialize` requests rematerialization with a non-zero `SubIdx`
(i.e. the consumer only needs a particular sub-lane of the original
def), `substituteRegister` will rewrite the new MOV32ri's operand 0
to `%DestReg.SubIdx`. But `MOV32ri` writes 32 bits; combined with a
SubIdx like `sub_8bit`, you get `%DestReg.sub_8bit = MOV32ri imm32`
— a 32-bit write tagged as a sub_8bit subregister write. The
MachineVerifier may or may not catch this depending on the target
register class of DestReg.

For the "happy" rematerialization path (cloning the original), the
original Orig already may have had `Orig.getOperand(0).getSubReg() ==
0` (MOV32r0 def doesn't carry a SubReg). Cloning preserves that, and
`substituteRegister` then transforms the def to a sub_X subreg.

The cloned `Expand2AddrUndef` produces XOR32rr later in expandPostRA,
which writes 32 bits — combined with a sub_8bit subreg def this is the
same shape concern.

In addition: if `SubIdx == X86::sub_8bit_hi` and `DestReg` is in
`GR32_NOREX`, `substituteRegister` will produce
`%dest_in_GR32_NOREX.sub_8bit_hi = MOV32ri ...`. The high byte of a
NOREX class is the AH/BH/CH/DH register, but the MOV32ri encoding
unconditionally writes the full 32-bit register, clobbering bytes the
caller did not ask to clobber.

The original TargetInstrInfo::reMaterialize handles the SubIdx case by
producing a sequence that respects the subreg. The X86 override should
likewise:

- If `SubIdx != 0` for the MOV32ri rewrite path, fall back to the
  generic clone path (which assumes the consumer constructs the
  correct subreg use), OR
- Refuse rematerialization (return without inserting; caller will
  insert a copy instead).

## Reproducer (sketch)
Construct a function where %2 = MOV32r0 (defs $eflags) is sunk across
an EFLAGS-clobbering boundary by RegisterCoalescer/LiveRangeShrink and
the only live use of %2 is as `%2.sub_8bit`. The rematerializer chooses
the MOV32ri replacement (EFLAGS not dead at insertion point) and
substitutes with SubIdx = sub_8bit. The resulting MIR:

```
%3:gr32.sub_8bit = MOV32ri 0
```

This either trips the MachineVerifier or, in release builds, produces
an instruction that writes 32 bits while the LIS/VLR data say only 8
bits are written. Subsequent passes (e.g. RegAllocFast) may allocate
under the 8-bit assumption and re-use the upper 24 bits — miscompile.

## Wrong outcome
A 32-bit MOV emitted where the caller asked for a sub-register def,
clobbering more of the destination than the LiveIntervals know about.
Real-world manifestation: extra dead bits set to zero, potentially
clobbering register state that subsequent code assumed preserved.

## Reproducer harness
```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
  -run-pass=greedy,virtregrewriter -verify-machineinstrs repro.mir
```
where repro.mir is shaped to force rematerialization with a subreg
operand and EFLAGS live at insertion site.
