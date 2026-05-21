; PSLLWri by 1 is mutated in-place to PADDWrr by X86FixupInstTuning, but the
; lambda that performs the rewrite returns `false`. The pass therefore reports
; PreservedAnalyses::all() to NPM (and `Changed = false` to legacy PM) even
; though the MIR was modified.

define <8 x i16> @shl_by_1(<8 x i16> %x) {
  %r = shl <8 x i16> %x, <i16 1, i16 1, i16 1, i16 1, i16 1, i16 1, i16 1, i16 1>
  ret <8 x i16> %r
}
