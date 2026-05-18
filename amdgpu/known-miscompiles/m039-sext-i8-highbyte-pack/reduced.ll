; RUN-INPUTS: 0*1,0x1
; RUN-LLVM-BUILD: build/llvm-fuzzer

source_filename = "m039-sext-i8-highbyte-pack.ll"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %wi to i64
  %salt = mul i32 %wi, -1640531527
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64

  %byte = and i32 %salt, 127
  %mul = mul i32 %byte, %byte
  %i8 = trunc i32 %mul to i8
  %sext = sext i8 %i8 to i32

  %lo.shr = lshr i32 %sext, 16
  %lo = and i32 %lo.shr, 255
  %salt.byte1 = and i32 %salt, 65280
  %or0 = or i32 %lo, %salt.byte1
  %mid = and i32 %sext, 16711680
  %or1 = or i32 %or0, %mid
  %hi = and i32 %salt, -16777216
  %result = or i32 %or1, %hi
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
