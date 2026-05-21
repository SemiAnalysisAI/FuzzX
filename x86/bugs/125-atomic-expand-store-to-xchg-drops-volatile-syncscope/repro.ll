target triple = "x86_64-unknown-linux-gnu"
define void @f1(ptr %p, i128 %x) {
  store atomic volatile i128 %x, ptr %p syncscope("singlethread") seq_cst, align 16
  ret void
}
