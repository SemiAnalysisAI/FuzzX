; Constant-fold check of fptoui/fptosi float->i1 semantics
define i32 @f2u_0()   { %r = fptoui float 0.0  to i1  %z = zext i1 %r to i32  ret i32 %z }
define i32 @f2u_1p5() { %r = fptoui float 1.5  to i1  %z = zext i1 %r to i32  ret i32 %z }
define i32 @f2u_0p5() { %r = fptoui float 0.5  to i1  %z = zext i1 %r to i32  ret i32 %z }
define i32 @f2s_0()   { %r = fptosi float 0.0  to i1  %z = zext i1 %r to i32  ret i32 %z }
define i32 @f2s_1p5() { %r = fptosi float 1.5  to i1  %z = zext i1 %r to i32  ret i32 %z }
define i32 @f2u_neg0() { %r = fptoui float -0.0 to i1 %z = zext i1 %r to i32  ret i32 %z }
