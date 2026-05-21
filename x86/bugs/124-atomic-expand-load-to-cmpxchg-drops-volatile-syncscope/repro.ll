target triple = "x86_64-unknown-linux-gnu"
define i128 @f1(ptr %p) {
  %v = load atomic volatile i128, ptr %p syncscope("singlethread") seq_cst, align 16
  ret i128 %v
}
