; c015: amdgcn.{raw,struct,struct.ptr}.buffer.load.format.i8 (and
; store) drop format encoding in SDAG -- emit non-format byte/short
; opcodes instead of buffer_load_format_x / buffer_store_format_x.
;
; SIISelLowering.cpp:7730-7732 (lowerIntrinsicLoad):
;
;   if (!IsD16 && !LoadVT.isVector() && EltType.getSizeInBits() < 32)
;     return handleByteShortBufferLoads(DAG, LoadVT, DL, Ops,
;                                       M->getMemOperand(), IsTFE);
;
; handleByteShortBufferLoads (line 12760) ignores the IsFormat flag
; and unconditionally emits AMDGPUISD::BUFFER_LOAD_UBYTE /
; BUFFER_LOAD_USHORT (or _TFE variants) -- non-format opcodes.  The
; buffer-rsrc format descriptor is therefore not applied to the
; loaded byte.
;
; The store mirror handleByteShortBufferStores (line 12798) emits
; BUFFER_STORE_BYTE / SHORT for the same reason
; (SIISelLowering.cpp:12151-12153, 12202-12205).
;
; i16 escapes the bug because the IsD16 early-return at line 7725
; routes 16-bit-element format loads to BUFFER_LOAD_FORMAT_D16 first.
;
; SDAG (broken):  buffer_load_ubyte v1, v0, s[0:3], 0 idxen
; GISel (correct): buffer_load_format_x v1, v0, s[0:3], 0 idxen
;
; Sibling of c011 (TFE chain-drop in vector illegal-type branch) and
; c014 (missing illegal-vector handling on tbuffer path).  Different
; code path: this is the byte/short scalar branch with
; format-encoding loss.

source_filename = "c015-buffer-load-format-i8-drops-format"
target triple = "amdgcn-amd-amdhsa"

declare i8 @llvm.amdgcn.struct.ptr.buffer.load.format.i8(
    ptr addrspace(8), i32, i32, i32, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc, ptr addrspace(1) %out) {
  %r = call i8 @llvm.amdgcn.struct.ptr.buffer.load.format.i8(
      ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0, i32 0)
  store i8 %r, ptr addrspace(1) %out
  ret void
}
