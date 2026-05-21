define i32 @volatile_idempotent_or(ptr %p) {
  %x = atomicrmw volatile or ptr %p, i32 0 seq_cst, align 4
  ret i32 %x
}
define i32 @nonvolatile_idempotent_or(ptr %p) {
  %x = atomicrmw or ptr %p, i32 0 seq_cst, align 4
  ret i32 %x
}
