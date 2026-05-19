; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0x0
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %wi to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %ov = call { i32, i1 } @llvm.umul.with.overflow.i32(i32 %v, i32 %wi)
  %value = extractvalue { i32, i1 } %ov, 0
  %overflow = extractvalue { i32, i1 } %ov, 1
  %overflow.i32 = zext i1 %overflow to i32
  %x = xor i32 %value, %overflow.i32
  %ab = and i32 %x, %x
  %ac = and i32 %x, 2
  %maj0 = or i32 %ab, %ac
  %majority = or i32 %maj0, %ac
  %maj.shl = shl i32 %majority, 1
  %result = xor i32 2, %maj.shl
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1
declare { i32, i1 } @llvm.umul.with.overflow.i32(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
!llvm.module.flags = !{!0, !1, !2}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
!1 = !{i32 1, !"amdgpu_printf_kind", !"hostcall"}
!2 = !{i32 8, !"PIC Level", i32 2}
