; RUN-INPUTS: 0x00000000
; RUN-LLVM-BUILD: build/llvm-fuzzer
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %idx = add i32 %n, %wi
  %hi = shl i32 2047, 16
  %x = add i32 %hi, %idx
  %ashr = ashr i32 %x, 8
  %a2.shr = lshr i32 %ashr, 16
  %a2 = and i32 %a2.shr, 255
  %b3 = and i32 %x, -16777216
  %b2.shr = lshr i32 %x, 8
  %b2 = and i32 %b2.shr, 65280
  %a3.shr = lshr i32 %ashr, 8
  %a3 = and i32 %a3.shr, 16711680
  %r01 = or i32 %b2, %b3
  %r012 = or i32 %r01, %a2
  %result = or i32 %r012, %a3
  store i32 %result, ptr addrspace(1) %out, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
