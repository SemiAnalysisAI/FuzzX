define i32 @vol_atomicrmw(ptr %p) {
  %old = atomicrmw volatile add ptr %p, i32 1 seq_cst, align 4
  ret i32 %old
}
define i32 @vol_cmpxchg(ptr %p) {
  %p1 = cmpxchg volatile ptr %p, i32 0, i32 1 seq_cst seq_cst, align 4
  %v = extractvalue { i32, i1 } %p1, 0
  ret i32 %v
}
