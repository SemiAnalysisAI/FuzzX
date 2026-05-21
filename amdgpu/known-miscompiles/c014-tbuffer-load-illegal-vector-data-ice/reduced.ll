; c014: `amdgcn.raw.ptr.tbuffer.load.v3i16` and friends ICE in SDAG
; on gfx950 -- missing illegal-vector handling on the non-D16 path.
;
; SIISelLowering.cpp:11394-11399 (raw) and :11421-11426 (struct) +
; mirror on store side :12053-12107.
;
; The D16 fast-path checks ONLY MVT::f16 scalar type and routes
; through adjustLoadValueType (which handles odd-lane widening).
; All other illegal vector return types -- <3 x i16>, <6 x i16>,
; <3 x bfloat> -- skip that branch and fall through to a plain
; getMemIntrinsicNode(AMDGPUISD::TBUFFER_LOAD_FORMAT, ..., LoadVT, ...)
; with the illegal-typed value.
;
; There is NO equivalent of the buffer.load.format illegal-type
; bitcast branch (lowerIntrinsicLoad lines 7739-7745).
;
; The illegal-typed INTRINSIC_W_CHAIN then reaches ReplaceNodeResults
; INTRINSIC_W_CHAIN (line 8256), which returns the still-illegal
; <3 x i16> value -- legalizer reports:
;
;   LLVM ERROR: Do not know how to widen the result of this operator!
;
; and aborts.

source_filename = "c014-tbuffer-load-illegal-vector-data-ice"
target triple = "amdgcn-amd-amdhsa"

declare <3 x i16> @llvm.amdgcn.raw.ptr.tbuffer.load.v3i16(
    ptr addrspace(8), i32, i32, i32 immarg, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc,
                             ptr addrspace(1) %out) {
  %r = call <3 x i16> @llvm.amdgcn.raw.ptr.tbuffer.load.v3i16(
      ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0, i32 0)
  store <3 x i16> %r, ptr addrspace(1) %out
  ret void
}
