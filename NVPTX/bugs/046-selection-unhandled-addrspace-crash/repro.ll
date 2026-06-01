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
