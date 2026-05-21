target triple = "x86_64-unknown-linux-gnu"
declare <4 x i32> @llvm.masked.expandload.v4i32(ptr, <4 x i1>, <4 x i32>)
define <4 x i32> @expand(ptr %p, <4 x i1> %m, <4 x i32> %pas) {
  %r = call <4 x i32> @llvm.masked.expandload.v4i32(ptr %p, <4 x i1> %m, <4 x i32> %pas), !nontemporal !0
  ret <4 x i32> %r
}
!0 = !{i32 1}
