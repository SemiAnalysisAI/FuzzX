; Reproduces AAAMDWavesPerEU propagation bug
; (AMDGPUAttributor.cpp:1165-1170): the attributor uses max() for BOTH
; bounds of waves-per-eu union, when the lower bound should be min().
; A callee shared by kernels with [1,1] and [8,8] should get [1,8] but
; gets [8,8] -- the high-occupancy kernel's tight register budget is
; imposed on the callee even when invoked from the relaxed kernel.
;
;   unsigned Min = std::max(Assumed.getLower(), CallerAA.getLower());  // WRONG
;   unsigned Max = std::max(Assumed.getUpper(), CallerAA.getUpper());
;
; Run with:
;   opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -passes=amdgpu-attributor \
;       -S reduced.ll
;
; Expected: callee gets "amdgpu-waves-per-eu"="1,8" (union)
; Observed: callee gets "amdgpu-waves-per-eu"="8,8" (tightest kernel)

source_filename = "m117-attributor-waves-per-eu-max-of-lower"
target triple = "amdgcn-amd-amdhsa"

define internal void @callee(ptr addrspace(1) %out, i32 %x) {
  store i32 %x, ptr addrspace(1) %out
  ret void
}

define amdgpu_kernel void @k_tight(ptr addrspace(1) %out, i32 %x) #0 {
  call void @callee(ptr addrspace(1) %out, i32 %x)
  ret void
}

define amdgpu_kernel void @k_relaxed(ptr addrspace(1) %out, i32 %x) #1 {
  call void @callee(ptr addrspace(1) %out, i32 %x)
  ret void
}

attributes #0 = { "amdgpu-waves-per-eu"="8,8" "target-cpu"="gfx950" }
attributes #1 = { "amdgpu-waves-per-eu"="1,1" "target-cpu"="gfx950" }
