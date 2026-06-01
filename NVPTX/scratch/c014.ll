target triple = "nvptx64-nvidia-cuda"

declare {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.row.stride.f16.p0(ptr, i32)

; Store to the upper half of the 32-byte fragment source, then wmma.load reading all 32 bytes.
define {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @t(ptr %p, i32 %s) {
  %hi = getelementptr i8, ptr %p, i64 16
  store <2 x half> <half 1.0, half 2.0>, ptr %hi, align 4
  %r = call {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.row.stride.f16.p0(ptr %p, i32 %s)
  ret {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} %r
}
