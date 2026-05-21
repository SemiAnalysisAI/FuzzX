target triple = "x86_64-unknown-linux-gnu"
declare void @use(ptr)
define void @f() "probe-stack"="inline-asm" {
  %a = alloca [4096 x i8], align 16
  call void @use(ptr %a)
  ret void
}
