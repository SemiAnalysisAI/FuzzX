target triple = "x86_64-pc-windows-msvc"
declare ptr @objc_retainAutoreleasedReturnValue(ptr)
declare ptr @foo()

define ptr @bar() {
  %r = call ptr @foo() [ "clang.arc.attachedcall"(ptr @objc_retainAutoreleasedReturnValue) ]
  ret ptr %r
}
