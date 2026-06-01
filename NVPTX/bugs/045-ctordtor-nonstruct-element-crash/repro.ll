target triple = "nvptx64-nvidia-cuda"
declare void @ctor()
@llvm.global_ctors = appending global [2 x { i32, ptr, ptr }] [
  { i32, ptr, ptr } { i32 65535, ptr @ctor, ptr null },
  { i32, ptr, ptr } zeroinitializer ]
