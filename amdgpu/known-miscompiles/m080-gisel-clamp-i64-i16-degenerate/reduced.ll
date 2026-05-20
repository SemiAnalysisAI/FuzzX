; RUN-INPUTS: 0x00000032,0x00000000
; RUN-LLVM-BUILD: build/llvm-fuzzer
;
; NOTE: This bug only manifests when compiling with
;   `-mllvm -global-isel`
; which is NOT the default flag set used by the AMDGPU clang driver, and is
; NOT passed by run_ll_reproducer.sh.  The standard reproducer script will
; therefore show O0 == O2 (both correct, value 5).
;
; To verify the bug manually:
;   clang -O0 -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx950 \
;       -mllvm -global-isel -S -x ir reduced.ll -o reduced.O0.gisel.s
;   # observe `v_cvt_pk_i16_i32` + `v_med3_i32 v?, 5, packed, 100` in the
;   # output -- this med3 yields 50 (the median), but the IR semantic is 5.
;
; The bug is in AMDGPUPreLegalizerCombiner.cpp `matchClampI64ToI16`:
; the validator accepts the constants if both fit in i16, but does not
; check that the (Cmp1, Cmp2) ordering actually matches the matched
; pattern. For pattern 1 = smin(smax(X, Cmp2), Cmp1), a valid clamp
; requires Cmp1 >= Cmp2; for pattern 2 = smax(smin(X, Cmp2), Cmp1), a
; valid clamp requires Cmp1 <= Cmp2.  When the inequality goes the wrong
; way the IR is a constant-Cmp1 expression, but the combiner rewrites it
; to med3(X_packed, min(Cmp1,Cmp2), max(Cmp1,Cmp2)) -- a real clamp that
; returns X when X falls inside [min, max].

target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %lo32 = load i32, ptr addrspace(1) %p0, align 4
  %hi32 = load i32, ptr addrspace(1) %p1, align 4
  %lo64 = zext i32 %lo32 to i64
  %hi64 = zext i32 %hi32 to i64
  %hishift = shl i64 %hi64, 32
  %origin = or i64 %lo64, %hishift
  ; Pattern 1: smin(smax(X, 100), 5) -- degenerate; always yields 5.
  %t0 = call i64 @llvm.smax.i64(i64 %origin, i64 100)
  %t1 = call i64 @llvm.smin.i64(i64 %t0, i64 5)
  %r16 = trunc i64 %t1 to i16
  %r32 = sext i16 %r16 to i32
  %idx64 = zext i32 %wi to i64
  %op = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %r32, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i64 @llvm.smax.i64(i64, i64)
declare i64 @llvm.smin.i64(i64, i64)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
