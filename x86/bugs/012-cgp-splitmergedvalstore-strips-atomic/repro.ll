target triple = "x86_64-unknown-linux-gnu"
; (int, FP) mix should trigger isMultiStoresCheaperThanBitsMerge
define void @atom_fp(ptr %p, i32 %lo, float %hi) {
  %hi_i = bitcast float %hi to i32
  %lo64 = zext i32 %lo to i64
  %hi64 = zext i32 %hi_i to i64
  %hishl = shl i64 %hi64, 32
  %merged = or i64 %lo64, %hishl
  store atomic i64 %merged, ptr %p seq_cst, align 8
  ret void
}
define void @nonatom_fp(ptr %p, i32 %lo, float %hi) {
  %hi_i = bitcast float %hi to i32
  %lo64 = zext i32 %lo to i64
  %hi64 = zext i32 %hi_i to i64
  %hishl = shl i64 %hi64, 32
  %merged = or i64 %lo64, %hishl
  store i64 %merged, ptr %p, align 8
  ret void
}
