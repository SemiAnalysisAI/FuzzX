; AMDGPURewriteOutArguments: two ptr arguments stored to, may alias.
;
; The pass treats both as "out" arguments and folds them into a struct
; return.  After rewrite, the stub stores the values in OutArg-index order,
; but the body packs them into the struct in the order MDA happened to
; report them.  Result: when the two args alias (or are simply not noalias),
; the stored values get swapped.
;
; Original semantics:
;   *%out0 := 1, *%out1 := 2
; After rewrite (see opt -amdgpu-rewrite-out-arguments output):
;   *%out0 := 2, *%out1 := 1
;
; Reproduce:
;   opt -S -amdgpu-rewrite-out-arguments \
;     -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 reduced.ll
;
; (The pass is not in the default codegen pipeline; it is invoked only
; through `opt` or third-party tooling that explicitly enables it.)

target triple = "amdgcn-amd-amdhsa"
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"

define void @foo(ptr addrspace(5) %out0, ptr addrspace(5) %out1) {
entry:
  store i32 1, ptr addrspace(5) %out0, align 4
  store i32 2, ptr addrspace(5) %out1, align 4
  ret void
}
