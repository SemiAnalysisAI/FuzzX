; m081: GISel selectWaveShuffleIntrin XORs ThreadID with the *shifted* index,
;       SDAG lowerWaveShuffle XORs ThreadID with the *unshifted* index.
;
; Discovery method: code inspection.
;
; The bug is in
; llvm/lib/Target/AMDGPU/AMDGPUInstructionSelector.cpp:selectWaveShuffleIntrin
; lines ~4082-4128: the same v_lshlrev_b32 result is reused as the
; operand to v_xor_b32 against the mbcnt_lo (ThreadID), so the check
; "(ThreadID ^ Index) & 32 == 0" is computed instead as
; "(ThreadID ^ (Index<<2)) & 32 == 0".  That is bit 5 of ThreadID XOR
; bit 3 of Index instead of bit 5 of Index.  The SDAG path in
; llvm/lib/Target/AMDGPU/SIISelLowering.cpp:lowerWaveShuffle XORs with
; the unshifted Index and is correct.
;
; The buggy code path only runs on wave64 targets without wave-wide
; bpermute support, i.e. GFX10/GFX11 in wavefrontsize64 mode.  Our
; local hardware is gfx950 (GFX9) which supportsWaveWideBPermute so
; the easy fast path is taken there.  Run on gfx1100 +wavefrontsize64
; or use llc directly to observe the buggy asm.
;
; To observe the asm divergence:
;   clang -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx1100 \
;       -Xclang -target-feature -Xclang +wavefrontsize64 \
;       -mllvm -global-isel -S -x ir reduced.ll -o reduced.gisel.s
;   clang -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx1100 \
;       -Xclang -target-feature -Xclang +wavefrontsize64 \
;       -S -x ir reduced.ll -o reduced.sdag.s
;   diff <(grep -E 'v_xor|v_lshlrev|v_mbcnt|v_and_b32_e64|v_cmp_eq|v_cndmask|ds_bpermute|v_permlane64' reduced.gisel.s) \
;        <(grep -E 'v_xor|v_lshlrev|v_mbcnt|v_and_b32_e64|v_cmp_eq|v_cndmask|ds_bpermute|v_permlane64' reduced.sdag.s)
;
; Runtime semantic divergence: lane 0 reading wave_shuffle(value, 8)
; where value[lane] = lane should yield 8 (SDAG: in-half, use bpermute).
; GISel's check "(ThreadID ^ (8<<2)) & 32 = 0 ^ 32 = 32" instead
; triggers the OUT-of-half path which goes through permlane64 first and
; returns value[8 XOR 32] = 40.

target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 %wi, %n
  br i1 %ok, label %body, label %exit

body:
  ; The wave_shuffle "value" at lane L is L itself, so the correct
  ; result of wave_shuffle(value, idx) at lane L is `idx[L]`.  We
  ; compute %v from the workitem id (no load).
  ;
  ; The shuffle index is constructed at runtime as
  ;   idx[L] = idx_lo XOR (L AND -8)
  ; with `idx_lo` loaded from memory so the compiler can't constant-
  ; fold the shuffle.  With idx_lo = 8, lane 0 reads source lane 8 --
  ; the simplest case where bit 5 of index = 0 (same half) but
  ; bit 3 of index = 1 (which the buggy GISel check incorrectly
  ; treats as "other half").
  %pi = getelementptr i32, ptr addrspace(1) %in, i32 0
  %idx_lo = load i32, ptr addrspace(1) %pi, align 4
  %lane_hi = and i32 %wi, -8
  %idx = xor i32 %idx_lo, %lane_hi
  %s = call i32 @llvm.amdgcn.wave.shuffle.i32(i32 %wi, i32 %idx)
  %op = getelementptr i32, ptr addrspace(1) %out, i32 %wi
  store i32 %s, ptr addrspace(1) %op, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.amdgcn.wave.shuffle.i32(i32, i32)

; GFX10/GFX11 in wavefrontsize64 mode triggers the buggy whole-wave path
; in selectWaveShuffleIntrin.  GFX9 (e.g. gfx950) and GFX12, and any
; wavefrontsize32 target, take a different fast path that does not have
; this bug.
attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx1100" "target-features"="+wavefrontsize64" "uniform-work-group-size"="true" }
