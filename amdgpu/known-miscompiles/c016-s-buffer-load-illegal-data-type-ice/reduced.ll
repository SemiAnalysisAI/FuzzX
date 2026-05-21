; c016: amdgcn.s.buffer.load.<T> ICEs in SDAG when result type T is
; illegal and not specifically handled (only i16 and v3-of-legal-scalar
; get widening).
;
; SIISelLowering.cpp:10537-10632 (lowerSBuffer):
;   - Only i16 (line 10559) and v3-of-legal-scalar (line 10567) get
;     illegal-type widening.
;   - All other illegal result types fall through to
;     getMemIntrinsicNode(AMDGPUISD::SBUFFER_LOAD, ..., VT, ...) at
;     line 10579 holding an illegal value type.
;   - Divergent path assertion at line 10604-10605 restricts scalars
;     to i32/f32 only.
;
; SIISelLowering.cpp:8199-8246 (ReplaceNodeResults for
; Intrinsic::amdgcn_s_buffer_load):
;   - Hard-asserts VT == MVT::i8 (line 8212).
;
; Reproducer matrix (gfx950, -O0 and -O2, both LLVM HEAD and ROCm 7.2.3):
;
;   i1            -> Cannot select SBUFFER_LOAD i1
;   i4            -> Do not know how to promote this operator
;   i24           -> Do not know how to promote this operator
;   <2 x i1>      -> Do not know how to split the result of this operator
;   <3 x i16>     -> Do not know how to widen the result of this operator
;   <6 x i8>      -> Do not know how to widen the result of this operator
;   i128          -> Do not know how to expand the result of this operator
;
; Sibling defect to c011 (TFE chain-drop), c014 (tbuffer illegal-vector),
; c015 (buffer.load.format.i8 drops format).

source_filename = "c016-s-buffer-load-illegal-data-type-ice"
target triple = "amdgcn-amd-amdhsa"

declare i128 @llvm.amdgcn.s.buffer.load.i128(<4 x i32>, i32, i32 immarg)

define amdgpu_kernel void @t(<4 x i32> %r, ptr addrspace(1) %o) {
  %v = call i128 @llvm.amdgcn.s.buffer.load.i128(<4 x i32> %r, i32 0, i32 0)
  store i128 %v, ptr addrspace(1) %o
  ret void
}
