; c018: amdgcn.image.atomic.<op>.<dim> ICEs or silently miscompiles
; in SDAG for any data width other than 32 or 64 bits.
;
; SIISelLowering.cpp:10156-10181 (lowerImage, atomic branch):
;   bool Is64Bit = VData.getValueSizeInBits() == 64;       (L10165)
;   ...
;   NumVDataDwords = Is64Bit ? 2 : 1;                      (L10180)
;   DMask          = Is64Bit ? 0x3 : 0x1;                  (L10179)
;
; The branch is a binary 32-vs-64 dispatch on the source data width
; and ignores every other case.  int_amdgcn_image_atomic_swap is
; declared llvm_any_ty, so the overload set includes <3 x i32>,
; <3 x i16>, <6 x i16>, <3 x bfloat>, i128, bfloat, etc.  All hit
; the Is64Bit==false arm with NumVDataDwords=1, DMask=0x1, and the
; MIMG selector picks the 1-dword V1 opcode regardless of the
; actual VData register-class width.
;
; Symptom matrix (gfx950, -O0 and -O2, LLVM HEAD + ROCm 7.2.3):
;
;   <3 x i32>     -> crash in copyPhysReg / MCInstPrinter
;   <3 x i16>     -> "Do not know how to widen the result"
;   <6 x i16>     -> "Do not know how to widen the result"
;   <3 x bfloat>  -> "Do not know how to widen the result"
;   i128          -> "Do not know how to expand the result"
;   bfloat        -> *** SILENT MISCOMPILE *** -- emits 1-dword
;                    image_atomic_swap dmask:0x1 with garbage upper
;                    16 bits of the dword overwriting the texel.
;   i128 cmpswap  -> "Cannot select: v2i32 = bitcast v4i64"
;
; bf16 is the most damaging: silent corruption of the texel image
; sibling of m142 (same MVT::f16-only check in non-atomic arms).
;
; Sibling family: c011/c014/c015/c016/c017/m142.

source_filename = "c018-image-atomic-illegal-data-type-ice"
target triple = "amdgcn-amd-amdhsa"

declare <3 x i32> @llvm.amdgcn.image.atomic.swap.1d.v3i32.i32(
    <3 x i32>, i32, <8 x i32>, i32, i32)

define amdgpu_kernel void @t(<8 x i32> inreg %r, i32 %x,
                             <3 x i32> %v, ptr addrspace(1) %o) {
  %res = call <3 x i32> @llvm.amdgcn.image.atomic.swap.1d.v3i32.i32(
      <3 x i32> %v, i32 %x, <8 x i32> %r, i32 0, i32 0)
  store <3 x i32> %res, ptr addrspace(1) %o
  ret void
}
