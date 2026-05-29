; LLVM IR `fmul X, 1.0` should perform an actual mulss. The IEEE-754 spec
; mandates that all arithmetic ops on sNaN return a qNaN. `SelectionDAG::
; simplifyFPBinop` (DAGCombiner/SelectionDAG.cpp:11584-11627) folds these
; identity cases to bare X without an `nnan` guard, so the asm becomes
; `retq` — no quieting performed.

define float @fmul_one(float %x) {
  %r = fmul float %x, 1.0
  ret float %r
}
define float @fdiv_one(float %x) {
  %r = fdiv float %x, 1.0
  ret float %r
}
