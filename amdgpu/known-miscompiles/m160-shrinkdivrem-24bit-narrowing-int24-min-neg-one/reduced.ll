; m160: shrinkDivRem64 24-bit narrowing of INT24_MIN / -1 produces
; wrong sign.  Direct sibling of m103 (32-bit boundary) and m132
; (vector composition).
;
; AMDGPUCodeGenPrepare.cpp:1354-1361 (shrinkDivRem64) and
; expandDivRem24Impl at AMDGPUCodeGenPrepare.cpp:1155-1162:
;
; For i64 sdiv whose LHS has bit pattern 0xFFFFFFFFFF800000 (i.e.
; sext i24 -2^23 or sext i32 -8388608 clipped) and RHS = -1:
;   ComputeNumSignBits(LHS) = 41, RHS = 64
;   getDivNumBits returns 64 - 41 + 1 = 24
;   expandDivRem24Impl fires.
;
; Inside expandDivRem24Impl:
;   - operands truncated to i32 (-2^23, -1)
;   - FP reciprocal computes the true quotient +2^23 = 0x00800000
;     (mathematically correct)
;   - sign-extends from 24-bit via SHL 8 ; AShr 8:
;       0x00800000 << 8 = 0x80000000
;       AShr 8         = 0xFF800000 = -2^23  (sign extension corrupts)
;   - final sext to i64 yields 0xFFFFFFFFFF800000 instead of the
;     true +8388608 = 0x0000000000800000.
;
; Same gate is reachable from i32 sdiv where LHS has >=9 sign bits
; (|abs| <= 2^23) and RHS = -1, since getDivNumBits for i32 returns
; 32 - 9 + 1 = 24.  So this also fires for plain i32 sdiv of
; (-2^23) / (-1) -- yielding -2^23 instead of +2^23.

source_filename = "m160-shrinkdivrem-24bit-narrowing-int24-min-neg-one"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @t_i32(ptr addrspace(1) %p) {
  ; (-2^23) / (-1) should be +2^23 = 8388608 = 0x00800000.
  ; Buggy: returns -2^23 = 0xFF800000.
  %a = sub i32 0, 8388608             ; -2^23
  %b = sub i32 0, 1                   ; -1
  %r = sdiv i32 %a, %b
  store i32 %r, ptr addrspace(1) %p
  ret void
}

define amdgpu_kernel void @t_i64(ptr addrspace(1) %p) {
  ; i64 sext'd from i24 -2^23, divided by -1.
  ; Quotient should be +2^23 = 0x0000000000800000.
  ; Buggy: returns 0xFFFFFFFFFF800000.
  %a = sub i64 0, 8388608             ; -2^23 in i64
  %b = sub i64 0, 1                   ; -1
  %r = sdiv i64 %a, %b
  store i64 %r, ptr addrspace(1) %p
  ret void
}
