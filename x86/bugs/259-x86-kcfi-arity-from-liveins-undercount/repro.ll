target triple = "x86_64-unknown-linux-gnu"
@sink = external global i32
; void(i32 event, ptr ctx): event (RDI) unused, ctx (RSI) used.
define dso_local void @handler(i32 noundef %event, ptr noundef %ctx) !kcfi_type !1 {
  %v = load i32, ptr %ctx
  store i32 %v, ptr @sink
  ret void
}
!llvm.module.flags = !{!0, !2}
!0 = !{i32 4, !"kcfi", i32 1}
!1 = !{i32 199571451}
!2 = !{i32 4, !"kcfi-arity", i32 1}
