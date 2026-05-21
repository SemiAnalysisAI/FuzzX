; m153: WholeWaveFunction prologue (gfx950) computes
; `EXEC = ~entryEXEC` instead of `EXEC = -1` when there are no
; WWM scratch / CSR spill registers.
;
; SIFrameLowering.cpp:1041-1048:
;
;   if (FuncInfo->isWholeWaveFunction()) {
;     // If we have already saved some WWM CSR registers, then the EXEC is
;     // already -1 and we don't need to do anything else. Otherwise, set
;     // EXEC to -1 here.
;     if (!ScratchExecCopy)
;       buildScratchExecCopy(LiveUnits, MF, MBB, MBBI, DL,
;                            /*IsProlog*/ true,
;                            /*EnableInactiveLanes*/ true);   // <-- BUG
;     else if (WWMCalleeSavedRegs.empty())
;       EnableAllLanes();
;   }
;
; buildScratchExecCopy(..., EnableInactiveLanes=true) (line 974-980)
; emits `S_XOR_SAVEEXEC_B{32,64} tmp, -1`.  ISA semantics:
;   tmp = EXEC; EXEC = -1 XOR EXEC = ~EXEC
;
; The comment claims "set EXEC to -1 here," but the actual result is
; `EXEC = ~entryEXEC` (the inactive-lane mask of the wave at function
; entry).
;
; Trigger: amdgpu_gfx_whole_wave function with no WWM CSR / scratch
; spills.  Easy to elicit with a trivial WWF body that has no
; strict.wwm MFMA chains.
;
; Effect: body executes with EXEC == ~entryEXEC; previously active
; lanes become inactive and vice versa.  Whole-wave semantic
; violated end-to-end.

source_filename = "m153-wholewave-func-prologue-exec-xor-not-mov"
target triple = "amdgcn-amd-amdhsa"

; A WholeWaveFunction with no WWM register usage.
define amdgpu_gfx_whole_wave i32 @t(i1 %active, ptr addrspace(1) %p, i32 %x) {
  ; Trivial body: store x.  No strict.wwm / set.inactive / MFMA, so
  ; FuncInfo has no WWMScratchRegs and no WWMCalleeSavedRegs.
  store i32 %x, ptr addrspace(1) %p
  ret i32 0
}
