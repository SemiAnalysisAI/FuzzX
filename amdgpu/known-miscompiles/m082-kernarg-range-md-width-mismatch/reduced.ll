; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: dword 0x000003ff  ; bytes [0]=a=0xff (out-of-range!), [1]=b=0x03, padding
; RUN-INPUTS: dword 0x00000000  ; padding to align next arg
; RUN-INPUTS: qword 0           ; ptr addrspace(1) %out (filled by harness)
;
; AMDGPULowerKernelArguments transplants the `range` ParamAttr from a
; sub-dword kernel argument onto the *widened* i32 kernarg load it
; substitutes for the argument.  The load is i32, but the metadata it
; carries is `!{i8 0, i8 4}` -- a Range MD whose operand type does NOT
; match the load type.  That is rejected by the IR verifier
; (Verifier.cpp:4602 "Range types must match instruction type!"), and
; if it slips past the verifier it tells downstream passes that the
; whole i32 load is in [0, 4) -- so bytes 1..3 of the kernarg segment
; (which contain `b`, padding, etc.) are assumed to be zero.

target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(i8 range(i8 0, 4) %a, i8 %b, ptr addrspace(1) %out) #0 {
entry:
  %za = zext i8 %a to i32
  %zb = zext i8 %b to i32
  %shl = shl i32 %zb, 8
  %sum = or i32 %shl, %za
  store i32 %sum, ptr addrspace(1) %out, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950" }
