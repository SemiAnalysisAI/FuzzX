target triple = "nvptx64-nvidia-cuda"

define i32 @plain(ptr %p, i32 %v) {
  %r = atomicrmw add ptr %p, i32 %v monotonic
  ret i32 %r
}
define i32 @rmw_block(ptr %p, i32 %v) {
  %r = atomicrmw add ptr %p, i32 %v syncscope("block") monotonic
  ret i32 %r
}
define i32 @rmw_device(ptr %p, i32 %v) {
  %r = atomicrmw max ptr %p, i32 %v syncscope("device") monotonic
  ret i32 %r
}
