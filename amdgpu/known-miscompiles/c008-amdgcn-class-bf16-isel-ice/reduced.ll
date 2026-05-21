; ICE on gfx950: amdgcn.class.bf16 ISel can't select.
;
; SIISelLowering.cpp:10931-10933 lowers Intrinsic::amdgcn_class to
; AMDGPUISD::FP_CLASS for any source FP VT including bf16, but there
; is no V_CMP_CLASS_BF16 instruction nor any VOPCClassPat64 pattern
; for bf16 in VOPCInstructions.td:1223-1229 (only _F16/_F32/_F64).
;
; Result: `LLVM ERROR: Cannot select: i1 = AMDGPUISD::FP_CLASS ... bf16`
;
; Same root surface as m118 (bf16 over-promise in isCanonicalized) but
; at the lowering layer rather than the canonical-property layer.
;
; Note: `llvm.is.fpclass.bf16` works fine -- it correctly bitcasts to
; i16 and expands to integer compares.  The amdgcn-specific intrinsic
; doesn't take that route.
;
; Run with:
;   llc -mtriple=amdgcn -mcpu=gfx950 -O0 reduced.ll
;
; Crashes at both -O0 and -O2 (pure ISel issue, no combine involvement).

source_filename = "c008-amdgcn-class-bf16-isel-ice"
target triple = "amdgcn-amd-amdhsa"

declare i1 @llvm.amdgcn.class.bf16(bfloat, i32)

define i1 @class_bf16(bfloat %x) {
  %r = call i1 @llvm.amdgcn.class.bf16(bfloat %x, i32 3)
  ret i1 %r
}
