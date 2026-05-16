; Minimized from fuzzer input sha1 33162d6ff2cc0b53f97e3fe0e8ef87dbafa2dbf8.
; RUN-INPUTS: 0x7fffffff
; Run with:
;   known-miscompiles/run_ll_reproducer.sh known-miscompiles/m001-ashr-i16-zext/reduced.ll

source_filename = "m001-ashr-i16-zext"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %workgroup = call i32 @llvm.amdgcn.workgroup.id.x()
  %workitem = call i32 @llvm.amdgcn.workitem.id.x()
  %base = mul i32 %workgroup, 256
  %idx = add i32 %base, %workitem
  %in.range = icmp ult i32 %idx, %n
  br i1 %in.range, label %body, label %exit

body:
  %idx64 = zext i32 %idx to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %x = load i32, ptr addrspace(1) %in.ptr, align 4
  %trunc = trunc i32 %x to i16
  %shift = ashr i16 %trunc, 8
  %zext = zext i16 %shift to i32
  %result = xor i32 %x, %zext
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %result, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

!llvm.module.flags = !{!0, !1, !2}

!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
!1 = !{i32 1, !"amdgpu_printf_kind", !"hostcall"}
!2 = !{i32 7, !"PIC Level", i32 2}
