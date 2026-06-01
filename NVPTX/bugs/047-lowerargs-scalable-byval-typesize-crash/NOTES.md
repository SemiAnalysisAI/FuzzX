# 047 — NVPTXLowerArgs copyByValParam: scalable-vector byval kernel param hits internal TypeSize 'scalable->fixed' fatal error at *getAllocationSize(DL)

- **Kind:** crash (fatal/UB)
- **Reachable via:** default llc, sm_90
- **Component:** NVPTXLowerArgs.cpp 403-405 (also reachable via copyFunctionByValArgs at 503-515)  (round-7 area `C03-pass-crash`)

## Summary

a scalable-vector `byval` kernel param hits the TypeSize scalable→fixed `reportFatalInternalError` in `copyByValParam`

## Mechanism / root cause

copyByValParam() computes `const auto ArgSize = *AllocA->getAllocationSize(DL);` (TypeSize) and passes it to `IRB.CreateMemCpy(AllocA, ..., ArgInParamAS, ..., ArgSize);`. When the byval param type is a scalable vector (e.g. <vscale x 4 x i32>), AllocaInst::getAllocationSize returns a scalable TypeSize; CreateMemCpy needs a fixed-width length, so the implicit `TypeSize::operator ScalarTy()` conversion fires `reportFatalInternalError("Cannot implicitly convert a scalable size to a fixed-width size in TypeSize::operator ScalarTy()")`. This is an internal/fatal abort (not a graceful target-capability diagnostic), and the input IR is well-formed and verifier-accepted (`opt -passes=verify` exits 0; byval of a sized scalable vector is legal IR). Distinct from README #008 (that is the ArgUseChecker no-op-default path -> llvm_unreachable in convertToParamAS); this is a different mechanism and crash site (copyByValParam / TypeSize, reached on the mutation/copy path). Confirmed stack frame #9 = copyByValParam.

## Trigger

A ptx_kernel with a `byval(<vscale x N x T>)` argument that is mutated/escaped so the safe-copy path (case 3, copyByValParam) is taken — e.g. a store through the byval pointer. Any sm (reproduced sm_90). (A read-only scalable byval instead crashes later in DAG selection, outside this pass.)

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
define ptx_kernel void @kern(ptr byval(<vscale x 4 x i32>) align 16 %p, ptr addrspace(1) %out) {
entry:
  store i32 1, ptr %p, align 4
  ret void
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.7).
