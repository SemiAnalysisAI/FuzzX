# 188 — EarlyCSE CSEs distinct `freeze` instructions of same operand

Component: EarlyCSE (in default O2 pipeline as `early-cse<memssa>`)

Sibling of #187 (standard GVN) and #136 (NewGVN). EarlyCSE's hash-and-replace logic treats two `freeze` instructions of the same operand as identical, eliminating one.

Per LangRef: "If the input is undef or poison, freeze returns an arbitrary, but fixed, value of type ty." Each distinct freeze instruction must independently choose its value.

## Reproducer

```ll
define i32 @test(i32 %x) {
  %s = shl i32 1, %x       ; poison when %x >= 32
  %f1 = freeze i32 %s
  %f2 = freeze i32 %s
  %d = sub i32 %f1, %f2
  ret i32 %d
}
```

After `opt -passes=early-cse -S`:

```ll
define i32 @test(i32 %x) {
  %f1 = freeze i32 %s
  ret i32 0
}
```

EarlyCSE eliminated `%f2`, replacing it with `%f1`, so `%d = %f1 - %f1 = 0`. Per spec, `%f1` and `%f2` may legally pick different values when `%x >= 32`, so `%d` can be non-zero.

`-passes=early-cse<memssa>` (the actual variant in default `-O2`) reproduces the same fold.

## Fix

Same as #187 — treat `freeze` as opaque for CSE (assign fresh ID per instance).
