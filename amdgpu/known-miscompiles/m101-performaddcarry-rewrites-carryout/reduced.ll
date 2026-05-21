; RUN-INPUTS: 0xffffffff, 0x00000001, 0x00000000, 0x00000000
; Reproduces the carry-out miscompile in
; SITargetLowering::performAddCarrySubCarryCombine
; (amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:17527).
;
; out[0] = (z+w) overflow value     (just to keep z,w live)
; out[1] = (x+y+cc) mod 2^32        (matches between O0 and O2)
; out[2] = carry-out of uaddo(x+y, cc)
;          O0: ((x+y) mod 2^32 + cc) >= 2^32   -- correct
;          O2: (x+y+cc) >= 2^32                -- buggy, includes carry from x+y
;
; With x=0xffffffff,y=1,z=0,w=0: cc=0, x+y wraps to 0, IR carry = 0,
; O2 fold produces carry = 1.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh findings/m100-uaddo-carry-add-fold-carryout/reduced.ll

source_filename = "m100-uaddo-carry-add-fold-carryout"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare {i32, i1} @llvm.uadd.with.overflow.i32(i32, i32)
declare noundef i32 @llvm.amdgcn.workitem.id.x() #1
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %workgroup = call i32 @llvm.amdgcn.workgroup.id.x()
  %workitem  = call i32 @llvm.amdgcn.workitem.id.x()
  %base      = mul i32 %workgroup, 256
  %idx       = add i32 %base, %workitem
  %in.range  = icmp eq i32 %idx, 0
  br i1 %in.range, label %body, label %exit

body:
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %x  = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %y  = load volatile i32, ptr addrspace(1) %p1
  %p2 = getelementptr i32, ptr addrspace(1) %in, i64 2
  %z  = load volatile i32, ptr addrspace(1) %p2
  %p3 = getelementptr i32, ptr addrspace(1) %in, i64 3
  %w  = load volatile i32, ptr addrspace(1) %p3

  ; cc = (z + w) overflow
  %zwo  = call {i32, i1} @llvm.uadd.with.overflow.i32(i32 %z, i32 %w)
  %cc   = extractvalue {i32, i1} %zwo, 1
  %ccv  = extractvalue {i32, i1} %zwo, 0
  store i32 %ccv, ptr addrspace(1) %out

  %xy     = add i32 %x, %y
  %cc_i32 = zext i1 %cc to i32
  %r      = call {i32, i1} @llvm.uadd.with.overflow.i32(i32 %xy, i32 %cc_i32)
  %v      = extractvalue {i32, i1} %r, 0
  %co     = extractvalue {i32, i1} %r, 1
  %co_i32 = zext i1 %co to i32

  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %v, ptr addrspace(1) %o1
  %o2 = getelementptr i32, ptr addrspace(1) %out, i64 2
  store i32 %co_i32, ptr addrspace(1) %o2
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
