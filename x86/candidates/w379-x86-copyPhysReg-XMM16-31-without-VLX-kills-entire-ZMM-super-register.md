# w379: X86InstrInfo::copyPhysReg upgrades XMM16-31 copies to full-ZMM moves and applies `KillSrc` to the super-register

## Component
`llvm/lib/Target/X86/X86InstrInfo.cpp` - `X86InstrInfo::copyPhysReg`, VR128 / VR256 (extended without VLX) branch.

## Where
- `llvm/lib/Target/X86/X86InstrInfo.cpp:4336-4360, 4372-4374`

```cpp
4331  } else if (X86::VR128XRegClass.contains(DestReg, SrcReg)) {
4332    if (HasVLX)
4333      Opc = X86::VMOVAPSZ128rr;
4334    else if (X86::VR128RegClass.contains(DestReg, SrcReg))
4335      Opc = HasAVX ? X86::VMOVAPSrr : X86::MOVAPSrr;
4336    else {
4337      // If this an extended register and we don't have VLX we need to use a
4338      // 512-bit move.
4339      Opc = X86::VMOVAPSZrr;
4340      const TargetRegisterInfo *TRI = &getRegisterInfo();
4341      DestReg =
4342          TRI->getMatchingSuperReg(DestReg, X86::sub_xmm, &X86::VR512RegClass);
4343      SrcReg =
4344          TRI->getMatchingSuperReg(SrcReg, X86::sub_xmm, &X86::VR512RegClass);
4345    }
4346  } else if (X86::VR256XRegClass.contains(DestReg, SrcReg)) {
4347    if (HasVLX)
4348      Opc = X86::VMOVAPSZ256rr;
4349    else if (X86::VR256RegClass.contains(DestReg, SrcReg))
4350      Opc = X86::VMOVAPSYrr;
4351    else {
4352      // If this an extended register and we don't have VLX we need to use a
4353      // 512-bit move.
4354      Opc = X86::VMOVAPSZrr;
4355      const TargetRegisterInfo *TRI = &getRegisterInfo();
4356      DestReg =
4357          TRI->getMatchingSuperReg(DestReg, X86::sub_ymm, &X86::VR512RegClass);
4358      SrcReg =
4359          TRI->getMatchingSuperReg(SrcReg, X86::sub_ymm, &X86::VR512RegClass);
4360    }
4361  } ...
4372  if (Opc) {
4373    BuildMI(MBB, MI, DL, get(Opc), DestReg)
4374        .addReg(SrcReg, getKillRegState(KillSrc));
4375    return;
4376  }
```

## Bug
When the caller wants to copy an XMM16-31 (or YMM16-31) physical register on AVX-512 without VLX, `copyPhysReg` widens the move to a full `VMOVAPSZrr` (ZMM<-ZMM, 512 bits). It rewrites both `DestReg` and `SrcReg` from the XMM/YMM sub-register to its `VR512` super-register, then at line 4373-4374 emits the move with `getKillRegState(KillSrc)` applied to **the super-register**.

`KillSrc` was passed in by the caller and reflects liveness of the *original* sub-register (XMM or YMM). Applying it to the **ZMM super** tells liveness/regalloc that the entire 512 bits of the ZMM die at this point. If only the lower 128/256 bits were dead but the upper lanes were still live (which is legal in MIR after partial moves into a ZMM), this kill incorrectly extends the kill across the upper lanes and can cause:

1. Use-after-kill (subsequent reads of the upper lanes appear to read a dead reg).
2. `MachineVerifier` warnings only in expensive-check builds.
3. Mis-scheduling / mis-coalescing in post-RA passes if they rely on live-range info derived from the kill flag.

The Dest side is also widened: `BuildMI(..., DestReg)` writes the entire ZMM, which (correctly) defines the upper lanes too. Combined with the over-broad kill on Src, both ends of the copy now treat the super-reg as the unit, which means the upper lanes' separate liveness intervals must be invalidated by callers - but they are not aware that the copy was widened.

## Why it slips today
- The pre-condition for *any* code in this branch to be reachable is `Subtarget.hasAVX512() && !Subtarget.hasVLX() && (DestReg|SrcReg in XMM16-31)`. That subtarget exists (Xeon Phi KNL is AVX-512 without VLX), but most binaries with XMM16+ also enable VLX.
- For typical IR, `copyPhysReg` between extended XMM regs without VLX *and* with upper-ZMM lanes live is rare because front-ends rarely produce mixed-width ZMM live-ranges deliberately.

## Trigger conditions
A copy of `xmm16`-`xmm31` (or `ymm16`-`ymm31`) with `KillSrc=true` while another sub-lane of the source ZMM (e.g., the upper 384 bits) is still live. Can arise from:

- Spill code: spill of `xmm16` is widened to 512-bit, but the upper lanes were not actually spilled.
- Coalescing: a vreg constrained to `VR128X` is copied while the parent ZMM holds other independently-tracked values.

Speculative .ll forcing some kind of cross-XMM moves on `-mattr=+avx512f,-avx512vl`:

```ll
target triple = "x86_64-unknown-linux-gnu"

declare <4 x float> @ext_xmm()
declare <16 x float> @ext_zmm()

define <16 x float> @mix(ptr %a, ptr %b) {
entry:
  %z = call <16 x float> @ext_zmm()
  %x = call <4 x float> @ext_xmm()
  %s = shufflevector <4 x float> %x, <4 x float> undef,
                     <16 x i32> <i32 0, i32 1, i32 2, i32 3,
                                 i32 undef, i32 undef, i32 undef, i32 undef,
                                 i32 undef, i32 undef, i32 undef, i32 undef,
                                 i32 undef, i32 undef, i32 undef, i32 undef>
  %r = shufflevector <16 x float> %z, <16 x float> %s,
                     <16 x i32> <i32 16, i32 17, i32 18, i32 19,
                                 i32 4, i32 5, i32 6, i32 7,
                                 i32 8, i32 9, i32 10, i32 11,
                                 i32 12, i32 13, i32 14, i32 15>
  ret <16 x float> %r
}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+avx512f,-avx512vl` on a function with sufficient register pressure to force `copyPhysReg` on an `xmm16-31` could exercise the widened-move + super-reg-kill case. Confirming an actual miscompile requires dumping LiveIntervals and walking for use-after-kill, which a `-verify-machineinstrs -expensive-checks` build will help with.

## Severity
Latent. Liveness-flag bug; observable only when the upper ZMM lanes are separately live across the copy point.

## Fix sketch
Either:
- Save the original sub-reg in the def/use operands as a sub-register reference instead of widening the operand registers (avoids over-broad kill).
- Or only apply `KillSrc` to the super-reg when the caller has already confirmed the entire ZMM dies (which it usually has *not*).

Possible patch (use a sub-reg encoding on both operands):

```cpp
const TargetRegisterInfo *TRI = &getRegisterInfo();
Register DestZmm = TRI->getMatchingSuperReg(DestReg, X86::sub_xmm, &X86::VR512RegClass);
Register SrcZmm  = TRI->getMatchingSuperReg(SrcReg,  X86::sub_xmm, &X86::VR512RegClass);
BuildMI(MBB, MI, DL, get(X86::VMOVAPSZrr))
    .addReg(DestZmm, RegState::Define, X86::sub_xmm)
    .addReg(SrcZmm,  getKillRegState(KillSrc) | RegState::Undef, X86::sub_xmm);
```

(Implicit ZMM def/use on the implicit operand list will then track the super-reg correctly.)

## Confidence
Medium. The widening-without-sub-reg-encoding pattern is real and the resulting kill is over-broad; manifesting it on default x86 -O2 requires the AVX-512F-without-VLX subtarget plus a specific live-range topology.
