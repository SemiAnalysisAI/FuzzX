; Reproduces silent miscompile of bf16->i64 fptosi.sat (and the unsigned
; sibling): AMDGPUTargetLowering::LowerFP_TO_INT_SAT
; (AMDGPUISelLowering.cpp:3979-3986) groups bf16 with f16 in the
; "saturate at i32 first, then ext to i64" shortcut:
;
;   if (DstVT == MVT::i64 &&
;       (SrcVT == MVT::f16 || SrcVT == MVT::bf16 || ...)) {
;     const SDValue Int32VTOp = DAG.getValueType(MVT::i32);
;     return DAG.getNode(OpOpcode, DL, DstVT, Src, Int32VTOp);
;   }
;
; That shortcut is sound for f16 (max finite = 65504, fits in i32) but
; wrong for bf16: bf16 shares f32's exponent range, so |x| up to 3.39e38.
; Values in [2^31, 2^63) silently saturate to INT32_MAX instead of
; returning the correct i64 (or INT64_MAX, for overflow).
;
; Test value: bf16 0x4f80 = 2^32 = 4294967296.
;   Expected (IR fptosi.sat semantics): 0x0000000100000000.
;   Observed (SDAG O0 & O2 on gfx950):  0x000000007fffffff.
;
; Bug is in Custom legalization (runs unconditionally), so O0 == O2.
; The witness is SDAG vs IR semantics; the FuzzX O0-vs-O2 oracle won't
; flag this.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m105-fptosisat-bf16-i64-clamps-at-i32/reduced.ll

source_filename = "m105-fptosisat-bf16-i64-clamps-at-i32"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare i64 @llvm.fptosi.sat.i64.bf16(bfloat)
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
  ; Volatile load the bf16 bits as i32 so the fold can't see the constant.
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi = load volatile i32, ptr addrspace(1) %p0
  %xh = trunc i32 %xi to i16
  %x  = bitcast i16 %xh to bfloat

  %r  = call i64 @llvm.fptosi.sat.i64.bf16(bfloat %x)
  %rlo = trunc i64 %r to i32
  %rhi64 = lshr i64 %r, 32
  %rhi = trunc i64 %rhi64 to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rlo, ptr addrspace(1) %o0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %rhi, ptr addrspace(1) %o1
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00004f80
; (bf16 0x4f80 = 2^32 = 4294967296; expected lo=0x00000000, hi=0x00000001;
;  observed (buggy) lo=0x7FFFFFFF, hi=0x00000000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
