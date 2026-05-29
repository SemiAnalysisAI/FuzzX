define void @fn1(ptr %x) gc "statepoint-example" {
  %r = addrspacecast ptr %x to ptr addrspace(1)
  call void @fn2(ptr addrspace(1) %r)
  ret void
}
declare void @fn2(ptr addrspace(1))
