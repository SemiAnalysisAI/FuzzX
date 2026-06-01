# 066 — f16 round-to-integer ops (ceil/floor/trunc/rint/nearbyint/roundeven) emit native cvt.*.f16.f16 on sm_50/sm_52 where f16 math is unsupported

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu=sm_50/sm_52
- **Component:** NVPTXISelLowering.cpp 956-967 (specifically 958); selection patterns in NVPTXInstrInfo.td 2432-2450 (CVT_ROUND -> CVT_f16_f16, line 602)  (round-8 area `X08-f16-bf16-arith-arch2`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration; no local `ptxas` was available to execute the rejection.

## Summary

f16 `ceil/floor/trunc/rint/nearbyint/roundeven` set Legal unconditionally (skipping `setFP16OperationAction`), emitting native `cvt.rpi.f16.f16` etc. on sm_50/sm_52 where f16 math needs sm_53

## Mechanism / root cause

In the rounding-op loop the f16 action is set unconditionally Legal:

  for (const auto &Op : {ISD::FCEIL, ISD::FFLOOR, ISD::FNEARBYINT, ISD::FRINT, ISD::FROUNDEVEN, ISD::FTRUNC}) {
    setOperationAction(Op, MVT::f16, Legal);   // line 958: bypasses setFP16OperationAction()
    ...
    setBF16OperationAction(Op, MVT::bf16, Legal, Promote);  // bf16 correctly gated
  }

Every other f16 FP operation in this file routes through the setFP16OperationAction lambda, which sets the action to NoF16Action (Promote) unless STI.allowFP16Math() (== hasFP16Math() && !NoF16Math, hasFP16Math()==SmVersion>=53). The round ops skip that lambda and are Legal even when allowFP16Math() is false. The matching TableGen patterns (CVT_ROUND, NVPTXInstrInfo.td:2432-2443, plus fnearbyint/frint at 2449-2450) select CVT_f16_f16, which is defined by CVT_FROM_ALL<"f16",B16> (line 602) with an EMPTY predicate list, so it carries no sm_53 guard either. Result: cvt.{rzi,rmi,rni,rpi}.f16.f16 is emitted on sm_50/sm_52.

WHY WRONG: cvt with rounding-to-integer in .f16->.f16 (e.g. cvt.rpi.f16.f16) is a half-precision arithmetic operation. Per the PTX ISA, the .f16 floating-point type 'is allowed only in conversions to and from .f32, .f64 types, in half precision floating point instructions and texture fetch instructions'; native half-precision math (as opposed to .f16<->.f32/.f64 storage conversions, which are available on sm_20+) requires sm_53 / PTX ISA 4.2 (this is exactly what hasFP16Math()==SmVersion>=53 encodes, and what the LLVM half-support patch D28540 states: 'fp16 math operations are supported on sm_53+ GPUs only'). On the declared .target sm_50 / .version 4.0 (and sm_52 / .version 4.1) ptxas rejects cvt.rpi.f16.f16. The deliberate fallback for unsupported f16 math is promotion to f32 -- which is exactly what fadd/fmul/fma/fabs do on the same target (they emit cvt.f32.f16 + f32 op, valid) -- so the round ops are the odd one out and clearly should have gone through the same Promote path.

## Trigger

Any of llvm.ceil/floor/trunc/rint/nearbyint/roundeven on a half value, compiled for nvptx64 with -mcpu=sm_50 or sm_52 (PTX 4.0/4.1). Reproduces at -O0 and -O2. <2 x half> variants reproduce too (scalarized to the same scalar f16 round). Even forcing -mattr=+ptx78 keeps the native cvt because the gap is the SM (f16 math hardware), not the PTX version; the declared .target stays sm_50.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define half @ceil_f16(half %a) {
  %r = call half @llvm.ceil.f16(half %a)
  ret half %r
}
define half @trunc_f16(half %a) {
  %r = call half @llvm.trunc.f16(half %a)
  ret half %r
}
define half @floor_f16(half %a) {
  %r = call half @llvm.floor.f16(half %a)
  ret half %r
}
define half @rint_f16(half %a) {
  %r = call half @llvm.rint.f16(half %a)
  ret half %r
}
define half @roundeven_f16(half %a) {
  %r = call half @llvm.roundeven.f16(half %a)
  ret half %r
}
declare half @llvm.ceil.f16(half)
declare half @llvm.trunc.f16(half)
declare half @llvm.floor.f16(half)
declare half @llvm.rint.f16(half)
declare half @llvm.roundeven.f16(half)
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_52 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.83). 
