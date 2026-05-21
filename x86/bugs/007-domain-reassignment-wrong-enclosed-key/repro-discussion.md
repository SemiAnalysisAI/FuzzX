No reliable executable repro yet. The IR pattern that exercises the
duplicate-closure walk is:

```ll
target triple = "x86_64-unknown-linux-gnu"
define i64 @f(i64 %a, i64 %b, i1 %c) #0 {
entry:
  br i1 %c, label %L, label %R
L:
  %al = and i64 %a, %b
  br label %J
R:
  %ar = or  i64 %a, %b
  br label %J
J:
  %m = phi i64 [ %al, %L ], [ %ar, %R ]
  %n = xor i64 %m, %a
  %o = and i64 %n, %b
  ret i64 %o
}
attributes #0 = { "target-features"="+avx512bw,+avx512dq" }
```

The bug is real (clear off-by-name in source), but the second-closure path
is currently caught by the `EnclosedInstrs` failsafe, so this IR compiles
correctly. To turn into a verifier crash you need an MIR repro that
constructs two closures whose members overlap but whose DefMIs do not
overlap — a less natural shape.
