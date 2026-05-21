target triple = "x86_64-unknown-linux-gnu"
declare void @llvm.masked.scatter.v4i32.v4p0(<4 x i32>, <4 x ptr>, i32, <4 x i1>)
define void @scatter_dyn(<4 x i32> %v, <4 x ptr> %ptrs, <4 x i1> %m) {
  call void @llvm.masked.scatter.v4i32.v4p0(<4 x i32> %v, <4 x ptr> %ptrs, i32 4, <4 x i1> %m), !nontemporal !0
  ret void
}
!0 = !{i32 1}
