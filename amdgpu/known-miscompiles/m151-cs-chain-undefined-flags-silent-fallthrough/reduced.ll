; m151: SITargetLowering::LowerCall silently accepts undefined
; `llvm.amdgcn.cs.chain` Flags values.
;
; SIISelLowering.cpp:4248-4266 dispatches on the Flags immarg with
; two recognized cases:
;   - FlagsValue.isZero()         -> error if extra args present
;   - FlagsValue.isOneBitSet(0)   -> DVGPR path; validates 3 extra args
;
; There is no else { lowerUnhandledCall(...) }.  Any other Flags
; value (e.g. 2, 3, 5, ...) falls through silently:
;
;   - UsesDynamicVGPRs stays false (SIISelLowering.cpp:4208).
;   - ChainCallSpecialArgs only contains exec; NumVGPRs/FallbackExec/
;     FallbackCallee are never pushed (loop at line 4264 is skipped).
;   - LowerCall then picks AMDGPUISD::TC_RETURN_CHAIN (non-DVGPR),
;     selecting SI_CS_CHAIN_TC_W32/W64 pseudo (SIInstructions.td:900-901)
;     instead of _DVGPR.
;   - CLI.Args still contains the trailing IR-level variadic args
;     (NumVGPRs etc.); they are dropped from the lowered call without
;     diagnostic.
;
; Result: caller-visible loss of fallback semantics for an immarg
; value the user supplied.  Sibling family of m145 (SI_TCRETURN_CHAIN
; arg lowering defects).

source_filename = "m151-cs-chain-undefined-flags-silent-fallthrough"
target triple = "amdgcn-amd-amdhsa"

declare amdgpu_cs_chain void @callee(<3 x i32> inreg, i32, i32, i32, i32)

define amdgpu_cs_chain void @t(<3 x i32> inreg %sgpr, i32 %vgpr,
                               i32 %exec_mask, i32 %num_vgprs,
                               i32 %fallback_exec, i32 %fallback_callee) {
  ; Flags = 2: not 0 (no extra args) and not 1 (DVGPR path).
  ; This is undefined per the documented contract but the lowering
  ; silently accepts it and drops the fallback args.
  call void (ptr, i32, <3 x i32>, i32, i32, ...)
    @llvm.amdgcn.cs.chain.p0.i32.v3i32.i32(
        ptr @callee,
        i32 %exec_mask,
        <3 x i32> %sgpr,
        i32 %vgpr,
        i32 2,                ; <-- Flags = 2 (undefined; no diagnostic)
        i32 %num_vgprs,
        i32 %fallback_exec,
        i32 %fallback_callee)
  unreachable
}

declare void @llvm.amdgcn.cs.chain.p0.i32.v3i32.i32(ptr, i32, <3 x i32>, i32, i32 immarg, ...)
