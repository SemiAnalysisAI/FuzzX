target triple = "nvptx64-nvidia-cuda"

declare {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.row.stride.f16.p0(ptr, i32)

; wmma.load reads [base,base+32). Then store to base+16 (upper half).
; A correct compiler must keep the store AFTER the load reads the old value at base+16.
; If MMO claims the load ends at base+16, the store could be hoisted before the load.
define void @t(ptr %p, i32 %s, ptr %out) {
  %r = call {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.row.stride.f16.p0(ptr %p, i32 %s)
  ; force use of an upper-half lane (element 5 -> byte offset 20..23, in the "under-reported" region)
  %e = extractvalue {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} %r, 5
  store <2 x half> %e, ptr %out, align 4
  ; store into the upper half of the fragment source AFTER the load
  %hi = getelementptr i8, ptr %p, i64 20
  store <2 x half> <half 7.0, half 8.0>, ptr %hi, align 4
  ret void
}
