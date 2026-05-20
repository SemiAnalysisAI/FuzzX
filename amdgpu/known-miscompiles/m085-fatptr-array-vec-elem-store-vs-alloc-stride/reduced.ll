target triple = "amdgcn-amd-amdhsa"

; Load an array of 2 x <3 x i32> via a buffer fat pointer (addrspace 7).
; Each vector element has store size 12 bytes but alloc size 16 bytes.
; The array therefore has elements at byte offsets 0 and 16; total 32 bytes.
; AMDGPULowerBufferFatPointers's per-element splitting in visitLoadImpl uses
; getTypeStoreSize (12) instead of getTypeAllocSize (16), so it produces
; loads at offsets 0 and 12 instead of 0 and 16.

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(7) %p, ptr addrspace(1) %out) {
entry:
  %v = load [2 x <3 x i32>], ptr addrspace(7) %p, align 16
  %e0 = extractvalue [2 x <3 x i32>] %v, 0
  %e1 = extractvalue [2 x <3 x i32>] %v, 1
  %e0p = getelementptr inbounds <3 x i32>, ptr addrspace(1) %out, i32 0
  %e1p = getelementptr inbounds <3 x i32>, ptr addrspace(1) %out, i32 1
  store <3 x i32> %e0, ptr addrspace(1) %e0p, align 16
  store <3 x i32> %e1, ptr addrspace(1) %e1p, align 16
  ret void
}
