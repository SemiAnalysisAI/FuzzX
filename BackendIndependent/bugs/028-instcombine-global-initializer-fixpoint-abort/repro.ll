target datalayout = "p:8:8"

@g = external global i8
@c = constant ptr getelementptr inbounds (i8, ptr @g, i64 1)

define i1 @global_initializer_fixpoint(ptr %p) {
  %alloca = alloca ptr
  call void @llvm.memcpy.p0.p0.i32(ptr %alloca, ptr @c, i32 0, i1 false)
  %load = load ptr, ptr %alloca
  %cmp = icmp eq ptr %p, %load
  ret i1 %cmp
}

declare void @llvm.memcpy.p0.p0.i32(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i32, i1 immarg)
