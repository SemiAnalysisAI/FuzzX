declare {<3 x i16>, i32} @llvm.amdgcn.struct.ptr.buffer.load.format.sl_v3i16i32s(ptr addrspace(8), i32, i32, i32, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc, ptr addrspace(1) %out, ptr addrspace(1) %status) {
  %r = call {<3 x i16>, i32} @llvm.amdgcn.struct.ptr.buffer.load.format.sl_v3i16i32s(
         ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0, i32 0)
  %data = extractvalue {<3 x i16>, i32} %r, 0
  %st   = extractvalue {<3 x i16>, i32} %r, 1
  store <3 x i16> %data, ptr addrspace(1) %out
  store i32 %st, ptr addrspace(1) %status
  ret void
}
