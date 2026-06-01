import sys
n=int(sys.argv[1]) if len(sys.argv)>1 else 60000
print('target triple="nvptx64-nvidia-cuda"')
print("declare i32 @llvm.nvvm.prmt(i32,i32,i32)")
print("define i32 @c(i32 %x){")
print("  %v0=call i32 @llvm.nvvm.prmt(i32 %x,i32 0,i32 17)")
for i in range(1,n): print(f"  %v{i}=call i32 @llvm.nvvm.prmt(i32 %v{i-1},i32 0,i32 17)")
print(f"  %a=and i32 %v{n-1},255"); print("  ret i32 %a"); print("}")
