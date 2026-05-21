target triple = "x86_64-unknown-linux-gnu"
declare void @llvm.masked.compressstore.v4i32(<4 x i32>, ptr, <4 x i1>)
define void @cstore(<4 x i32> %v, ptr %p, <4 x i1> %m) {
  call void @llvm.masked.compressstore.v4i32(<4 x i32> %v, ptr %p, <4 x i1> %m), !nontemporal !0
  ret void
}
!0 = !{i32 1}
