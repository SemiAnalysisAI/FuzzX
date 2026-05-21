; Reproduces aperture-blind promotion in AMDGPUPromoteKernelArguments
; (AMDGPUPromoteKernelArguments.cpp:105-128). The pass wraps every
; FLAT_ADDRESS kernel-arg pointer in addrspacecast(addrspacecast(p to
; ptr addrspace(1)) to ptr) so InferAddressSpaces converts downstream
; memops to global_*. There is no check that the flat pointer is
; actually in the global aperture. A flat kernarg can legitimately
; carry a pointer whose aperture is LOCAL (AS 3) or PRIVATE (AS 5).
;
; Per LangRef, addrspacecast to a non-containing AS is poison; the
; AMDGPU lowering discards the aperture base, so the resulting
; "global" address is garbage and the subsequent global_store hits
; arbitrary global memory instead of the actual LDS/private slot.
;
; Run with:
;   opt -S -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 \
;       -passes='amdgpu-promote-kernel-arguments,infer-address-spaces' \
;       reduced.ll

source_filename = "m114-promotekernargs-flat-to-global-unconditional"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @k(ptr addrspace(1) readonly %pp, ptr %dst) #0 {
  ; %dst is a flat kernarg pointer.  In a real client the host may have
  ; populated it with `addrspacecast(@LDS to ptr)`, in which case the
  ; underlying aperture is AS 3 (LOCAL), not AS 1 (GLOBAL).
  store i32 7, ptr %dst, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950" }
