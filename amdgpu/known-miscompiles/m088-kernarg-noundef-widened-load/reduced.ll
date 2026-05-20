; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0x00000000
;
; AMDGPULowerKernelArguments stamps !noundef on the *widened* i32 kernarg
; load whenever the sub-dword argument carried a `noundef` ParamAttr.
; The widened load reads the entire dword that contains the sub-dword arg,
; so the high bits of the load come from a *different* kernarg (or from
; padding bytes) whose noundef-ness is NOT covered by the attribute.
;
; In this reduced kernel:
;   %a : i8 noundef  -> low byte of dword 0
;   %b : i1          -> bit 8 of dword 0 (NO noundef attribute)
;
; The widened load picks up `!noundef` because of %a's attribute. GVN +
; InstCombine then observes that the i32 load is noundef, concludes via
; `isGuaranteedNotToBeUndefOrPoison` that *every* bit (including bit 8
; that recovers %b) is noundef, and DROPS the `freeze i1 %b` guarding
; the branch.
;
; Source-level the program is well-defined (the freeze guarantees the
; branch sees a non-poison value, even if %b is poison at the kernel-arg
; boundary). After the optimizer's wrong-deduction, the kernel branches
; on a possibly-poison value -- which per LLVM IR semantics is
; immediate UB and licenses arbitrary downstream miscompiles.

target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(i8 noundef %a, i1 %b, ptr addrspace(1) %out) #0 {
entry:
  %za = zext i8 %a to i32
  store i32 %za, ptr addrspace(1) %out, align 4
  %fb = freeze i1 %b
  br i1 %fb, label %then, label %else
then:
  %p1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 1, ptr addrspace(1) %p1, align 4
  ret void
else:
  %p2 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 2, ptr addrspace(1) %p2, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950" }
