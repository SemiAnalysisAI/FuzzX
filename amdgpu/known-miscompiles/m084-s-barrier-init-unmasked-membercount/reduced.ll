; SDAG miscompile of llvm.amdgcn.s.barrier.init dynamic memberCount.
; gfx12+ only intrinsic, so this cannot be run on the gfx950 HIP harness;
; the SDAG vs GISel asm divergence below is itself the proof.
;
;   llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx1200 -global-isel=false reduced.ll
;     => s_lshl_b32 m0, s0, 16          ; <-- WRONG (no mask)
;        s_barrier_init m0
;
;   llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx1200 -global-isel=true  reduced.ll
;     => s_and_b32  s0, s0, 63          ; mask member count to 6 bits
;        s_lshl_b32 m0, s0, 16
;        s_barrier_init m0
;
; Per AMD gfx12 docs, M0[21:16] = memberCount (6 bits) and M0[5:0] = barrierID.
; With dynamic memberCount %cnt, SDAG forwards the un-masked %cnt into m0,
; allowing bits %cnt[15:6] to leak into m0 bits [31:22] (above the legal
; field). For any %cnt >= 64 the in-hardware named-barrier interprets a
; bogus member count.
target triple = "amdgcn-amd-amdhsa"

declare void @llvm.amdgcn.s.barrier.init(ptr addrspace(3), i32)
@bar = internal addrspace(3) global i32 poison, align 16

define amdgpu_kernel void @fuzz_kernel(i32 %cnt) #0 {
  call void @llvm.amdgcn.s.barrier.init(ptr addrspace(3) @bar, i32 %cnt)
  ret void
}
attributes #0 = { nounwind "target-cpu"="gfx1200" }
