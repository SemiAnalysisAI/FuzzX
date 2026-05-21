; Reproduces silent miscompile of f64 llvm.log2 / llvm.log / llvm.log10:
; AMDGPUISelLowering.cpp:419-427 marks FEXP/FEXP2/FEXP10 Custom for f64
; but NOT FLOG/FLOG2/FLOG10.  The f64 log family falls through generic
; Expand -> libcall, but AMDGPU has no libcall for `flogX`.  llc prints
; "error: no libcall available for flog2" yet exits 0 with a kernel that
; stores v0=0,v1=undef (the entire log call silently disappears).
;
; Under `strictfp` the same input HARD CRASHES llc.
;
; Affects gfx900, gfx950, gfx1100 (target-agnostic legality table).
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m102-f64-flog-silent-undef/reduced.ll

source_filename = "m102-f64-flog-silent-undef"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare double @llvm.log2.f64(double)
declare noundef i32 @llvm.amdgcn.workitem.id.x() #1
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %workgroup = call i32 @llvm.amdgcn.workgroup.id.x()
  %workitem  = call i32 @llvm.amdgcn.workitem.id.x()
  %base      = mul i32 %workgroup, 256
  %idx       = add i32 %base, %workitem
  %in.range  = icmp eq i32 %idx, 0
  br i1 %in.range, label %body, label %exit

body:
  ; load f64 = bitcast of two i32 inputs (RUN-INPUTS uses i32 words)
  %p0  = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xlo = load volatile i32, ptr addrspace(1) %p0
  %p1  = getelementptr i32, ptr addrspace(1) %in, i64 1
  %xhi = load volatile i32, ptr addrspace(1) %p1
  %xlo64 = zext i32 %xlo to i64
  %xhi64 = zext i32 %xhi to i64
  %xhish = shl i64 %xhi64, 32
  %xbits = or i64 %xhish, %xlo64
  %x     = bitcast i64 %xbits to double

  %r     = call double @llvm.log2.f64(double %x)
  %rbits = bitcast double %r to i64
  %rlo64 = trunc i64 %rbits to i32
  %rhi64 = lshr i64 %rbits, 32
  %rhi   = trunc i64 %rhi64 to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rlo64, ptr addrspace(1) %o0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %rhi, ptr addrspace(1) %o1
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00000000, 0x40000000
; (x = 0x4000000000000000 = 2.0, log2(2.0) = 1.0 = 0x3FF0000000000000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
