; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0,0,0
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %idx64 = zext i32 %wi to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4

  ; Build x and y bytes
  %x.lo = and i32 %v, 255
  %y.lo = and i32 %wi, 255
  %y.nz = or i32 %y.lo, 1

  ; Compute usub.with.overflow on tiny byte values
  %ov.call = call { i32, i1 } @llvm.usub.with.overflow.i32(i32 %x.lo, i32 %y.nz)
  %ov.value = extractvalue { i32, i1 } %ov.call, 0
  %ov.bit = extractvalue { i32, i1 } %ov.call, 1
  %ov.i32 = zext i1 %ov.bit to i32

  ; Mimic the buggy byte gather chain
  %lane.xor = xor i32 %ov.value, %ov.i32
  %fold.add = add i32 0, %lane.xor
  %fold = xor i32 %fold.add, %ov.i32
  %byte.xor = xor i32 %lane.xor, %fold
  %byte = and i32 %byte.xor, 1

  store i32 %byte, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare { i32, i1 } @llvm.usub.with.overflow.i32(i32, i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
