; RUN-INPUTS: 0x0ccf4d8b*256
; RUN-REPEAT: 200
; RUN-COMBINED: 1
; RUN-LLVM-BUILD: build/rocm-7.2.3-llvm-cov-release

; ModuleID = 'fuzzx_amdgpu_diff'
source_filename = "fuzzx_amdgpu_diff"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %0 = call i32 @llvm.amdgcn.workgroup.id.x()
  %1 = call i32 @llvm.amdgcn.workitem.id.x()
  %2 = mul i32 %0, 256
  %3 = add i32 %2, %1
  %4 = icmp ult i32 %3, %n
  br i1 %4, label %body, label %exit

body:                                             ; preds = %entry
  %5 = zext i32 %3 to i64
  %6 = getelementptr i32, ptr addrspace(1) %in, i64 %5
  %7 = load i32, ptr addrspace(1) %6, align 4
  %8 = add i32 %7, -1453987328
  %9 = add i32 %8, 531191274
  %10 = alloca [4 x i32], align 4, addrspace(5)
  %11 = add i32 %9, -568350379
  %12 = xor i32 %9, -568350379
  %13 = mul i32 %3, -568350379
  %14 = add i32 %13, %9
  %15 = getelementptr [4 x i32], ptr addrspace(5) %10, i32 0, i32 0
  store i32 %9, ptr addrspace(5) %15, align 4
  %16 = getelementptr [4 x i32], ptr addrspace(5) %10, i32 0, i32 1
  store i32 %11, ptr addrspace(5) %16, align 4
  %17 = getelementptr [4 x i32], ptr addrspace(5) %10, i32 0, i32 2
  store i32 %12, ptr addrspace(5) %17, align 4
  %18 = getelementptr [4 x i32], ptr addrspace(5) %10, i32 0, i32 3
  store i32 %14, ptr addrspace(5) %18, align 4
  %19 = getelementptr [4 x i32], ptr addrspace(5) %10, i32 0, i32 1
  %20 = load i32, ptr addrspace(5) %19, align 4
  %21 = and i32 %20, 1860050857
  %22 = bitcast i32 %21 to <4 x i8>
  %23 = call <4 x i8> @llvm.cttz.v4i8(<4 x i8> %22, i1 false)
  %24 = bitcast <4 x i8> %23 to i32
  %25 = xor i32 %21, %24
  %26 = and i32 %25, 28382
  %27 = zext i32 %26 to i64
  %28 = call { i64, i1 } @llvm.ssub.with.overflow.i64(i64 %27, i64 -2441046286548336530)
  %29 = extractvalue { i64, i1 } %28, 0
  %30 = extractvalue { i64, i1 } %28, 1
  %31 = zext i1 %30 to i32
  %32 = trunc i64 %29 to i32
  %33 = lshr i64 %29, 32
  %34 = trunc i64 %33 to i32
  %35 = add i32 %32, %34
  %36 = shl i32 %31, 14
  %37 = xor i32 %35, %36
  %38 = mul i32 %37, 0
  %39 = add i32 %38, -1453987328
  %40 = add i32 %39, 531191274
  %41 = alloca [4 x i32], align 4, addrspace(5)
  %42 = add i32 %40, -568350379
  %43 = xor i32 %40, -568350379
  %44 = mul i32 %3, -568350379
  %45 = add i32 %44, %40
  %46 = getelementptr [4 x i32], ptr addrspace(5) %41, i32 0, i32 0
  store i32 %40, ptr addrspace(5) %46, align 4
  %47 = getelementptr [4 x i32], ptr addrspace(5) %41, i32 0, i32 1
  store i32 %42, ptr addrspace(5) %47, align 4
  %48 = getelementptr [4 x i32], ptr addrspace(5) %41, i32 0, i32 2
  store i32 %43, ptr addrspace(5) %48, align 4
  %49 = getelementptr [4 x i32], ptr addrspace(5) %41, i32 0, i32 3
  store i32 %45, ptr addrspace(5) %49, align 4
  %50 = getelementptr [4 x i32], ptr addrspace(5) %41, i32 0, i32 1
  %51 = load i32, ptr addrspace(5) %50, align 4
  %52 = and i32 %51, 1860050857
  %53 = bitcast i32 %52 to <4 x i8>
  %54 = call <4 x i8> @llvm.cttz.v4i8(<4 x i8> %53, i1 false)
  %55 = bitcast <4 x i8> %54 to i32
  %56 = xor i32 %52, %55
  %57 = and i32 %56, 28382
  %58 = zext i32 %57 to i64
  %59 = call { i64, i1 } @llvm.ssub.with.overflow.i64(i64 %58, i64 -2441046286548336530)
  %60 = extractvalue { i64, i1 } %59, 0
  %61 = extractvalue { i64, i1 } %59, 1
  %62 = zext i1 %61 to i32
  %63 = trunc i64 %60 to i32
  %64 = lshr i64 %60, 32
  %65 = trunc i64 %64 to i32
  %66 = add i32 %63, %65
  %67 = shl i32 %62, 14
  %68 = xor i32 %66, %67
  %69 = getelementptr i32, ptr addrspace(1) %out, i64 %5
  store i32 %68, ptr addrspace(1) %69, align 4
  br label %exit

exit:                                             ; preds = %body, %entry
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare <4 x i8> @llvm.cttz.v4i8(<4 x i8>, i1 immarg) #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare { i64, i1 } @llvm.ssub.with.overflow.i64(i64, i64) #1

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

!llvm.module.flags = !{!0, !1, !2}

!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
!1 = !{i32 1, !"amdgpu_printf_kind", !"hostcall"}
!2 = !{i32 7, !"PIC Level", i32 2}
