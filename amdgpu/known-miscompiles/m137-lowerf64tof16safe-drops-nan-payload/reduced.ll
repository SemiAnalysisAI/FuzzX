; RUN-INPUTS: 0xabcd1234, 0x7ff1bcde
; RUN-LLVM-BUILD: build/llvm-fuzzer
; lo16 = LowerF64ToF16Safe direct; hi16 = via_f32 (HW). For NaN with
; payload in low half (0x7ff1bcde_abcd1234), the direct AMDGPU
; expansion yields 0x7e00 while the HW chain yields 0x7e6f.
target triple = "amdgcn-amd-amdhsa"

@volatile_a = internal global i64 0
@volatile_b = internal global i64 0

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %lo32 = load i32, ptr addrspace(1) %in, align 4
  %php = getelementptr i32, ptr addrspace(1) %in, i32 1
  %hi32 = load i32, ptr addrspace(1) %php, align 4
  %lo64 = zext i32 %lo32 to i64
  %hi64 = zext i32 %hi32 to i64
  %hisl = shl i64 %hi64, 32
  %xi = or i64 %hisl, %lo64
  ; Make the two doubles separate to defeat CSE: use two volatile loads
  store volatile i64 %xi, ptr @volatile_a, align 8
  store volatile i64 %xi, ptr @volatile_b, align 8
  %xi_a = load volatile i64, ptr @volatile_a, align 8
  %xi_b = load volatile i64, ptr @volatile_b, align 8
  %xd_dir = bitcast i64 %xi_a to double
  %xd_via = bitcast i64 %xi_b to double
  ; Direct path
  %r_dir = fptrunc double %xd_dir to half
  ; Via f32 explicit
  %xf = fptrunc double %xd_via to float
  %r_via = fptrunc float %xf to half
  %ri_dir = bitcast half %r_dir to i16
  %ri_via = bitcast half %r_via to i16
  %z_dir = zext i16 %ri_dir to i32
  %z_via = zext i16 %ri_via to i32
  %sh_dir = shl i32 %z_dir, 16
  %combined = or i32 %sh_dir, %z_via
  %op = getelementptr i32, ptr addrspace(1) %out, i32 %wi
  store i32 %combined, ptr addrspace(1) %op, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
