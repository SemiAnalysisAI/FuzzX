; Reproduces miscompile in AMDGPUPreloadKernelArguments where the
; explicit-arg `inreg`-marking loop's budget check ignores
; `BaseOffset = ST.getExplicitKernelArgOffset()`, so on non-AMDHSA /
; non-AMDPAL / non-Mesa3D triples (where BaseOffset = 36 bytes) the
; pass over-marks explicit kernel arguments as `inreg`.
;
; Code references (post-LLVM-21):
;   amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/
;     AMDGPUPreloadKernelArguments.cpp:181-183  <-- canPreloadKernArgAtOffset
;       return ExplicitArgOffset <= NumFreeUserSGPRs * 4;
;     AMDGPUPreloadKernelArguments.cpp:295-329  <-- markKernelArgsAsInreg
;       uint64_t ExplicitArgOffset = 0;                             // <-- BUG
;       const uint64_t BaseOffset = ST.getExplicitKernelArgOffset();
;       ...
;       ExplicitArgOffset = alignTo(...) + AllocSize;
;       if (!PreloadInfo.canPreloadKernArgAtOffset(ExplicitArgOffset))
;         break;                                                    // <-- BUG
;     AMDGPUPreloadKernelArguments.cpp:240-242  <-- hidden-arg path (correct)
;       canPreloadKernArgAtOffset(LoadOffset + LoadSize + ImplicitArgsBaseOffset)
;     AMDGPUPreloadKernelArguments.cpp:333-336  <-- BaseOffset folded in
;       uint64_t ImplicitArgsBaseOffset =
;           alignTo(ExplicitArgOffset, ST.getAlignmentForImplicitArgPtr()) +
;           BaseOffset;
;
;   AMDGPUSubtarget.h:254-265                   <-- BaseOffset = 36 for !HSA/!PAL/!Mesa3D
;   SIISelLowering.cpp:3061-3138                <-- allocatePreloadKernArgSGPRs
;     unsigned LastExplicitArgOffset = Subtarget->getExplicitKernelArgOffset();
;     // does account for BaseOffset, so SI-lowering's SGPR-budget check
;     // bails out earlier than the IR pass thought; bailed-out args remain
;     // `inreg` in IR.
;   AMDGPULowerKernelArguments.cpp:247-249      <-- "Skip inreg args"
;     if (Arg.use_empty() || Arg.hasInRegAttr()) continue;
;
; ---- demonstrated divergence ----------------------------------------------
;
; With this IR ran through:
;
;   opt -mtriple=amdgcn-- -mcpu=gfx950 \
;       -amdgpu-kernarg-preload-count=16 \
;       -passes='amdgpu-preload-kernel-arguments,function(amdgpu-lower-kernel-arguments)' \
;       -S reduced.ll
;
; the pass marks `%out` and `%a0..%a5` as `inreg`, identical to the
; AMDHSA case.  AMDHSA's kernarg segment really starts at offset 0,
; so the runtime preload writes the right SGPRs.  Unknown-OS's
; kernarg segment starts at byte 36 of the input buffer, so the
; runtime preload reads bytes [0..32) of the buffer into the SGPRs
; that the kernel later treats as `%out`, `%a0..%a5` -- 32 bytes of
; runtime-header / pre-segment garbage.
;
;   llc -mtriple=amdgcn-- -mcpu=gfx950 \
;       -amdgpu-kernarg-preload-count=16 reduced.ll -o -
;
; emits:
;   s_load_dwordx8 s[8:15],  s[4:5], 0x0   ; <-- loads bytes 0..31 of input
;                                          ;     buffer into the SGPRs the
;                                          ;     kernel uses as %out + %a0..%a5
;   s_load_dwordx8 s[16:23], s[4:5], 0x44  ; 0x44 = 36 + 32 = 68 -- correct
;                                          ; offset for %a6..%aD (SI lowering
;                                          ; bailed here, so AMDGPULowerKernelArguments
;                                          ; rewrote them as kernarg-segment loads)
;
; vs the AMDHSA build of the same kernel which emits:
;   s_load_dwordx8 s[8:15],  s[4:5], 0x0   ; correct -- segment really starts at 0
;   s_load_dwordx8 s[16:23], s[4:5], 0x20  ; 0x20 = 32 -- correct for AMDHSA
;
; (`amdgcn--` has TargetTriple.getOS() == Triple::UnknownOS, which
; AMDGPUSubtarget.h:260-264 routes to the BaseOffset = 36 case.)
;
; This reproducer is at the IR/opt + llc level -- the FuzzX
; run_ll_reproducer.sh harness always targets amdgcn-amd-amdhsa, so
; this divergence is not visible from the HIP-runtime O0-vs-O2 path.
; Inspect with `opt` and `llc` directly.

source_filename = "m113-preloadkernargs-explicit-budget-omits-baseoffset"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn--"

; 16 i32 explicit args -> 64 bytes of explicit-arg payload, well within
; gfx950's NumFreeUserSGPRs * 4 budget when measured from offset 0, but
; over budget when measured from offset 36 (the true non-HSA start).
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out,
                                       i32 %a0, i32 %a1, i32 %a2, i32 %a3,
                                       i32 %a4, i32 %a5, i32 %a6, i32 %a7,
                                       i32 %a8, i32 %a9, i32 %aA, i32 %aB,
                                       i32 %aC, i32 %aD, i32 %aE) #0 {
entry:
  %p0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %a0, ptr addrspace(1) %p0, align 4
  %p1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %a1, ptr addrspace(1) %p1, align 4
  %p2 = getelementptr i32, ptr addrspace(1) %out, i64 2
  store i32 %a2, ptr addrspace(1) %p2, align 4
  %p3 = getelementptr i32, ptr addrspace(1) %out, i64 3
  store i32 %a3, ptr addrspace(1) %p3, align 4
  %p4 = getelementptr i32, ptr addrspace(1) %out, i64 4
  store i32 %a4, ptr addrspace(1) %p4, align 4
  %p5 = getelementptr i32, ptr addrspace(1) %out, i64 5
  store i32 %a5, ptr addrspace(1) %p5, align 4
  %p6 = getelementptr i32, ptr addrspace(1) %out, i64 6
  store i32 %a6, ptr addrspace(1) %p6, align 4
  %p7 = getelementptr i32, ptr addrspace(1) %out, i64 7
  store i32 %a7, ptr addrspace(1) %p7, align 4
  %p8 = getelementptr i32, ptr addrspace(1) %out, i64 8
  store i32 %a8, ptr addrspace(1) %p8, align 4
  %p9 = getelementptr i32, ptr addrspace(1) %out, i64 9
  store i32 %a9, ptr addrspace(1) %p9, align 4
  %pA = getelementptr i32, ptr addrspace(1) %out, i64 10
  store i32 %aA, ptr addrspace(1) %pA, align 4
  %pB = getelementptr i32, ptr addrspace(1) %out, i64 11
  store i32 %aB, ptr addrspace(1) %pB, align 4
  %pC = getelementptr i32, ptr addrspace(1) %out, i64 12
  store i32 %aC, ptr addrspace(1) %pC, align 4
  %pD = getelementptr i32, ptr addrspace(1) %out, i64 13
  store i32 %aD, ptr addrspace(1) %pD, align 4
  %pE = getelementptr i32, ptr addrspace(1) %out, i64 14
  store i32 %aE, ptr addrspace(1) %pE, align 4
  ret void
}

attributes #0 = { nounwind "amdgpu-no-implicitarg-ptr" "target-cpu"="gfx950" }
