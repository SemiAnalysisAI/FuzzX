; Reproduces value corruption in AMDGPUPrintfRuntimeBinding for
; `printf("%f", half_val)` with negative half operand
; (AMDGPUPrintfRuntimeBinding.cpp:191-203).
;
; The pass widens vararg printf args whose getTypeAllocSize is not a
; multiple of 4.  For scalar half / bfloat the path is:
;
;   tmp_i16 = bitcast half %h to i16
;   tmp_i32 = sext i16 %tmp_i16 to i32     ; <-- BUG: should be zext
;   stored as i32 in the printf record
;
; For negative half values (sign bit set), sext flips the top 16 bits
; of the i32 slot.  The runtime reads the i32 from the printf record
; and interprets it as an f32 for `%f`.  The corrupted high half makes
; it a wildly wrong f32.
;
; Example: half -2.0 has bit pattern 0xC000.
;   sext 0xC000 to i32 = 0xFFFFC000  (i32 value -16384)
;   zext 0xC000 to i32 = 0x0000C000  (the correct i32 lift of the half bits)
;
; The runtime's `%f` formatter then reads 0xFFFFC000 as f32 bits
; (= -169086976.0f) instead of the intended representation.  Even with
; the correct lift, half->f32 reinterpret would still need an fpext,
; but the SEXT version is unambiguously wrong: it cannot represent the
; original half value under ANY format the runtime might choose
; (whereas zext at least preserves the half bit pattern that the
; runtime could fpext-on-the-fly).
;
; Companion to m112 (size off-by-4 for `%s` with strlen%4==0).
;
; This reproducer is at the IR/opt level; runtime divergence requires
; actually running printf to observe the wrong character output.
;
; Run with:
;   opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 \
;       -passes=amdgpu-printf-runtime-binding -S reduced.ll

source_filename = "m125-printfbinding-half-sext-corrupts-fp-bits"
target triple = "amdgcn-amd-amdhsa"

@.fmt = private unnamed_addr addrspace(4) constant [4 x i8] c"%f\0A\00"

declare i32 @printf(ptr addrspace(4), ...)

define amdgpu_kernel void @k(half %h) {
  ; %h is a negative half, e.g. -2.0 (bit pattern 0xC000).
  ; Pass widens to i32 via bitcast+sext -> stores corrupted bits.
  call i32 (ptr addrspace(4), ...) @printf(ptr addrspace(4) @.fmt, half %h)
  ret void
}
