; RUN-INPUTS: 0x0*256
; RUN-LLVM-BUILD: build/llvm-fuzzer
;
; Strategy-divergence reproducer for llvm.amdgcn.wave.reduce.fsub.f64.
;
; Each lane reads an all-zero u32 from %in, zero-extends to i64, bitcasts to
; double (= +0.0).  The kernel then runs two wave.reduce.fsub strategies on
; that value and XORs the two i64 bit-patterns.  The result's high 32 bits are
; stored to out[tid].
;
; For all-zero input on gfx950 (wave64):
;   * Strategy 1 (ITERATIVE) returns +0.0  (i64 bits 0x0000000000000000)
;   * Strategy 2 (DPP)       returns -0.0  (i64 bits 0x8000000000000000)
; so the XOR's high half is 0x80000000, indicating the two strategies disagree
; on the sign of zero.  Both -O0 and -O2 reproduce the same wrong XOR, so the
; harness reports mismatch=false between opt levels, but the stored value
; itself (0x80000000) proves the strategy soundness bug.

target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %tid    = call i32 @llvm.amdgcn.workitem.id.x()
  %tid64  = zext i32 %tid to i64
  %inp    = getelementptr i32, ptr addrspace(1) %in, i64 %tid64
  %u32    = load i32, ptr addrspace(1) %inp, align 4
  %u64    = zext i32 %u32 to i64
  %v      = bitcast i64 %u64 to double
  %r_iter = call double @llvm.amdgcn.wave.reduce.fsub.f64(double %v, i32 1)
  %r_dpp  = call double @llvm.amdgcn.wave.reduce.fsub.f64(double %v, i32 2)
  %b_iter = bitcast double %r_iter to i64
  %b_dpp  = bitcast double %r_dpp  to i64
  %diff   = xor i64 %b_iter, %b_dpp
  %hi64   = lshr i64 %diff, 32
  %hi     = trunc i64 %hi64 to i32
  %outp   = getelementptr i32, ptr addrspace(1) %out, i64 %tid64
  store i32 %hi, ptr addrspace(1) %outp, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x() #1
declare double @llvm.amdgcn.wave.reduce.fsub.f64(double, i32 immarg) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { convergent nocallback nofree nounwind willreturn memory(none) }
