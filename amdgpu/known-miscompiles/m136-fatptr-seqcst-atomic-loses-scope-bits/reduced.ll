target triple = "amdgcn-amd-amdhsa"

; seq_cst on addrspace(7), with various syncscopes.
define amdgpu_kernel void @rmw_sys(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(7) %p, i32 %v seq_cst, align 4
  ret void
}

define amdgpu_kernel void @rmw_agent(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(7) %p, i32 %v syncscope("agent") seq_cst, align 4
  ret void
}

define amdgpu_kernel void @rmw_wg(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(7) %p, i32 %v syncscope("workgroup") seq_cst, align 4
  ret void
}

define amdgpu_kernel void @rmw_one_as(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(7) %p, i32 %v syncscope("agent-one-as") seq_cst, align 4
  ret void
}

; Same patterns on addrspace(1) for reference.
define amdgpu_kernel void @rmw_sys_g(ptr addrspace(1) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(1) %p, i32 %v seq_cst, align 4
  ret void
}

define amdgpu_kernel void @rmw_agent_g(ptr addrspace(1) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(1) %p, i32 %v syncscope("agent") seq_cst, align 4
  ret void
}
