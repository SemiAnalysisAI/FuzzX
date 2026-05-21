; c017: amdgcn.{raw,struct}.ptr.buffer.atomic.* ICEs in SDAG for
; illegal data types.  Sibling of c011/c014/c015/c016.
;
; SIISelLowering.cpp:
;   - lowerRawBufferAtomicIntrin    11196-11222 (mem node at 11219-11221)
;   - lowerStructBufferAtomicIntrin 11224-11250 (mem node at 11247-11249)
;   - cmpswap raw arm               11541-11563
;   - cmpswap struct arm            11565-11587
;
; Unlike lowerIntrinsicLoad's 7739-7745 (buffer.load.format), there is
; no CastVT/bitcast fallback for illegal vector/scalar value types in
; the atomic paths.  All four atomic lowerings hand the user-typed
; value straight to getMemIntrinsicNode.
;
; Reproducer matrix (gfx950, -O0 and -O2, both LLVM HEAD and ROCm 7.2.3):
;
;   raw.ptr.buffer.atomic.add.v3i16     -> widen result
;   raw.ptr.buffer.atomic.add.i128      -> expand result
;   raw.ptr.buffer.atomic.swap.v6i8     -> widen result
;   raw.ptr.buffer.atomic.swap.i24      -> segfault (no LLVM_ERROR)
;   raw.ptr.buffer.atomic.fadd.bf16     -> Cannot select AMDGPUISD::BUFFER_ATOMIC_FADD bf16
;   struct.ptr.buffer.atomic.add.v3i16  -> widen result
;   struct.ptr.buffer.atomic.cmpswap.i128 -> expand result

source_filename = "c017-buffer-atomic-illegal-data-type-ice"
target triple = "amdgcn-amd-amdhsa"

declare <3 x i16> @llvm.amdgcn.raw.ptr.buffer.atomic.add.v3i16(
    <3 x i16>, ptr addrspace(8), i32, i32, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc, <3 x i16> %v,
                             ptr addrspace(1) %out) {
  %r = call <3 x i16> @llvm.amdgcn.raw.ptr.buffer.atomic.add.v3i16(
      <3 x i16> %v, ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0)
  store <3 x i16> %r, ptr addrspace(1) %out
  ret void
}
