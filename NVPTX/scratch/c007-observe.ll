target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }

; byval %p: each thread should get a PRIVATE copy initialized from the param bank.
; Thread does: old = atomicRMW(add, p.field0, 7); then reads p.field0 again; stores both to out.
; With correct byval semantics every thread sees old = initial, new = initial+7,
; and the param bank is NEVER modified (private copy).
define ptx_kernel void @kern_observe(ptr byval(%struct.S) align 8 %p, i1 %c, ptr addrspace(1) %out) {
entry:
  %sel = select i1 %c, ptr %p, ptr %p
  %old = atomicrmw add ptr %sel, i32 7 seq_cst
  %reread = load i32, ptr %sel
  %o0 = getelementptr i32, ptr addrspace(1) %out, i32 0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i32 1
  store i32 %old, ptr addrspace(1) %o0
  store i32 %reread, ptr addrspace(1) %o1
  ret void
}
