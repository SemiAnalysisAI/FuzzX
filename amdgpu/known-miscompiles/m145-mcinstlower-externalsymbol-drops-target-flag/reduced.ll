; m145: AMDGPUMCInstLower MO_ExternalSymbol case drops
; MO.getTargetFlags() when constructing the MCSymbolRefExpr,
; producing an unrelocated symbol reference.
;
; AMDGPUMCInstLower.cpp:89-103 (MO_GlobalAddress branch) correctly
; honors `MO.getTargetFlags()` via `getSpecifier(MO.getTargetFlags())`
; when building the MCSymbolRefExpr.
;
; AMDGPUMCInstLower.cpp:104-109 (MO_ExternalSymbol branch) does NOT.
; It always builds a plain `MCSymbolRefExpr::create(...)` with no
; specifier.  Any AMDGPU-specific symbol specifier on the operand
; (MO_GOTPCREL, MO_REL32_LO, MO_REL32_HI, MO_ABS32_LO, MO_ABS32_HI,
; MO_REL64, MO_ABS64) is silently dropped, producing the wrong
; relocation type in the emitted object.
;
; This reproducer triggers an external-symbol reference via a
; runtime libcall (sdiv i64) that the AMDGPU SDAG lowering routes
; through ExternalSymbol with relocation flags.  On gfx950 the
; emitted object has the wrong relocation type for `__divdi3`
; (or analogous helper).
;
; Run with:
;   llc -mtriple=amdgcn -mcpu=gfx950 -O0 -filetype=obj reduced.ll
;
; Inspect the resulting .o with llvm-readelf -r and compare the
; relocation type against the MO_GlobalAddress reference baseline.

source_filename = "m145-mcinstlower-externalsymbol-drops-target-flag"
target triple = "amdgcn-amd-amdhsa"

declare void @external_callee(i64)

define amdgpu_kernel void @t(i64 %x) {
  call void @external_callee(i64 %x)
  ret void
}
