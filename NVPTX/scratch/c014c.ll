target triple = "nvptx64-nvidia-cuda"

declare {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.row.stride.f16.p0(ptr, i32)
declare {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.b.row.stride.f16.p0(ptr, i32)

; Two loads from the same base separated by a store to the upper half.
; load1 reads [base,base+32); store base+20; load2 reads [base,base+32).
; If MMO under-reports load1 to [base,base+16), the store could be hoisted above load1.
define void @t(ptr %p, i32 %s, ptr %o1, ptr %o2) {
  %r = call {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.row.stride.f16.p0(ptr %p, i32 %s)
  %e5 = extractvalue {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} %r, 5
  store <2 x half> %e5, ptr %o1, align 4
  %hi = getelementptr i8, ptr %p, i64 20
  store <2 x half> <half 7.0, half 8.0>, ptr %hi, align 4
  ret void
}
