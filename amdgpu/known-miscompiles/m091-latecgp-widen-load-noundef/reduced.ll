target triple = "amdgcn-amd-amdhsa"

; Show that the unsound !noundef can be weaponised: after the widen, we
; extract a different byte (offset+2) from the SAME i32 load, then freeze
; it and use it as a branch condition. With !noundef on the widened load,
; instcombine will drop the freeze (because the value derived from a
; "fully noundef" load is itself noundef), even though the only thing the
; source program promised noundef about was the byte at offset+1.

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(4) align 4 %p, ptr addrspace(1) %out) #0 {
entry:
  %p1 = getelementptr inbounds i8, ptr addrspace(4) %p, i64 1
  %v  = load i8, ptr addrspace(4) %p1, align 1, !noundef !0
  %vz = zext i8 %v to i32
  store i32 %vz, ptr addrspace(1) %out, align 4

  ; Now consume a DIFFERENT byte of the surrounding dword via a sibling
  ; load (which after widening will CSE with the widened load above).
  %p2 = getelementptr inbounds i8, ptr addrspace(4) %p, i64 2
  %w  = load i8, ptr addrspace(4) %p2, align 1
  %fw = freeze i8 %w
  %cmp = icmp ne i8 %fw, 0
  br i1 %cmp, label %then, label %else
then:
  %p3 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 1, ptr addrspace(1) %p3, align 4
  ret void
else:
  %p4 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 2, ptr addrspace(1) %p4, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950" "uniform-work-group-size"="true" }

!0 = !{}
