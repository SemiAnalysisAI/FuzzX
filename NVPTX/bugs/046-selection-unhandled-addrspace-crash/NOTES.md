# 046 — Load/store/atomicrmw through an unhandled pointer address space hits llvm_unreachable in NVPTXDAGToDAGISel::getAddrSpace

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXISelDAGToDAG.cpp 499-514 (unreachable at 513); callers at 564, 1121, 1199, 1395, 1442, 2272  (round-7 area `C02-selection-crash`)

## Summary

a load/store/atomicrmw through an unhandled pointer address space hits an `llvm_unreachable` in NVPTXISelDAGToDAG

## Mechanism / root cause

getAddrSpace() does `auto AS = static_cast<NVPTX::AddressSpace>(N->getMemOperand()->getAddrSpace());` then `switch (AS) { case Generic/Global/Shared/Const/Local/SharedCluster/EntryParam/DeviceParam: return AS; } llvm_unreachable("Unexpected address space");`. The NVPTX::AddressSpace enum only enumerates the address-space numbers 0 (Generic), 1 (Global), 3 (Shared), 4 (Const), 5 (Local), 7 (SharedCluster), 101 (EntryParam) and DeviceParam (see NVPTX.h:202-215, NVPTXAddrSpace.h:22-30). Any other numeric address space that is perfectly legal in LLVM IR -- e.g. addrspace(2), addrspace(6) (ADDRESS_SPACE_TENSOR), addrspace(8) -- falls through the switch and aborts via llvm_unreachable. This is reached unconditionally at the top of tryLoad (1121), tryLoadVector (1199), tryStore (1395), tryStoreVector (1442), the atomic path at 2272, and the helper at 564, so scalar loads, vector loads, plain stores, vector stores and atomicrmw on such pointers all crash. Note the sibling case at tryStoreVector:1444 correctly uses report_fatal_error (a graceful diagnostic) for the const-store case, so the intent for unsupported situations is clearly a clean error, not an llvm_unreachable abort. Arbitrary numeric address spaces are well-defined, non-UB LLVM IR (LangRef), so this is a compiler crash from valid input, not a clean 'cannot select'.

## Trigger

Run llc for an nvptx64 target on a function that loads from or stores to a pointer in an address space whose number is not one of {0,1,3,4,5,7,101}. addrspace(2), addrspace(6), addrspace(8) all reproduce. Reproduces with the default -mcpu as well as sm_90; scalar load, <4 x i32> vector load, and `load atomic ... monotonic` all crash identically.

## Reproducer

```
define i32 @load_as2(ptr addrspace(2) %p) {
  %v = load i32, ptr addrspace(2) %p
  ret i32 %v
}
; also crashes:
;   store i32 %v, ptr addrspace(2) %p
;   load <4 x i32>, ptr addrspace(2) %p, align 16
;   load atomic i32, ptr addrspace(2) %p monotonic, align 4
;   atomicrmw add ptr addrspace(2) %p, i32 1 monotonic
;   load i32, ptr addrspace(6) %p   ; ADDRESS_SPACE_TENSOR
;   load i32, ptr addrspace(8) %p
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.92).
