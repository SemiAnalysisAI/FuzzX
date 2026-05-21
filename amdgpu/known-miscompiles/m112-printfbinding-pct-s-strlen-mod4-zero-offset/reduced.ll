; Reproduces AMDGPUPrintfRuntimeBinding miscompile when a `%s` argument
; points to a string whose strlen is a positive multiple of 4.
;
; `AMDGPUPrintfRuntimeBinding.cpp:220` sizes the `%s` slot in the printf
; metadata as
;   ArgSize = alignTo(strlen + 1, 4)
; so for strlen == 4 ("abcd") the metadata advertises an 8-byte slot.
;
; The store loop (`AMDGPUPrintfRuntimeBinding.cpp:357-389, 401-415`) then
; reads `S.size()` bytes (== strlen, the NUL is *not* in `S`), packs them
; into one or more iN values, stores each one, and advances the next-arg
; GEP by `getTypeAllocSize` of the stored value. For "abcd" that is a
; single i32 store and a +4 GEP -- a 4-byte slot in IR, not 8.
;
; Net: every subsequent argument is stored at metadata_offset - 4. The
; HSA printf runtime walks the buffer using the metadata, so the
; integer argument that follows `%s` is read from a slot the kernel
; never wrote (zero-on-alloc / stale / next-printf-record bytes), and
; the value the kernel *did* store is interpreted as the next field's
; high bytes.
;
; This is a layout bug visible in IR / asm; it does not depend on the
; HSA printf runtime being available, so the standard hip_module_runner
; cannot directly observe the wrong printf output. The asm divergence
; (metadata advertises +12 for `%d`, IR stores `%d` at +8) is shown in
; NOTES.md alongside `opt -passes=amdgpu-printf-runtime-binding -S`.
;
; Run:
;   /opt/rocm-7.1.1/lib/llvm/bin/opt -mtriple=amdgcn-amd-amdhsa \
;       -mcpu=gfx950 -passes=amdgpu-printf-runtime-binding -S reduced.ll
;
; Inputs (RUN-INPUTS) feed printf's %d argument via a volatile load so
; the bug shape is exercised on whatever value the harness supplies;
; the kernel also stores a non-zero sentinel into %out so the
; FuzzX-format runner produces output bytes for the divergence script.
;
; RUN-INPUTS: 0xdeadbeef
; RUN-LLVM-BUILD: build/llvm-fuzzer

source_filename = "m112-printfbinding-pct-s-strlen-mod4-zero-offset"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

@.fmt = private unnamed_addr addrspace(4) constant [6 x i8] c"%s %d\00"
@.str = private unnamed_addr addrspace(4) constant [5 x i8] c"abcd\00"

declare i32 @printf(ptr addrspace(4), ...) #2

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %workgroup = call i32 @llvm.amdgcn.workgroup.id.x()
  %workitem = call i32 @llvm.amdgcn.workitem.id.x()
  %base = mul i32 %workgroup, 256
  %idx = add i32 %base, %workitem
  %in.range = icmp ult i32 %idx, %n
  br i1 %in.range, label %body, label %exit

body:
  %idx64 = zext i32 %idx to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  ; Volatile load so the printf argument cannot be hoisted / constant-folded
  ; away before AMDGPUPrintfRuntimeBinding sees it.
  %x = load volatile i32, ptr addrspace(1) %in.ptr, align 4
  %call = call i32 (ptr addrspace(4), ...) @printf(
      ptr addrspace(4) @.fmt,
      ptr addrspace(4) @.str,
      i32 %x)
  ; Store a sentinel so the FuzzX-format runner produces non-zero output
  ; and so DCE doesn't elide the kernel body. The sentinel is the value
  ; we *would* expect printf's runtime to read for %d if the layout were
  ; correct.
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %x, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nounwind }

!llvm.module.flags = !{!0, !1, !2}

!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
!1 = !{i32 1, !"amdgpu_printf_kind", !"hostcall"}
!2 = !{i32 7, !"PIC Level", i32 2}
