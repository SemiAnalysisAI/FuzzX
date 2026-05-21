# x86 GISel: i128 SUB inverts borrow into high word (`selectUAddSub` family)

GlobalISel lowers `sub i128 %a, %b` to `subq` (low) + `sbbq` (high), but
re-materializes CF before the `sbbq` by doing `setb %sil ; cmpb $1, %sil`.
That `cmpb` sets `CF = (sil < 1) = !borrow`, i.e. the borrow is inverted.
The high word is therefore off by 1 whenever the low subtraction does NOT
borrow, and correct only when it does borrow.

## IR
```llvm
define i128 @sub_i128(i128 %a, i128 %b) {
  %r = sub i128 %a, %b
  ret i128 %r
}
```

## Commands
```sh
llc -O0 -mtriple=x86_64-linux-gnu                sub128.ll -o sdag.s
llc -O0 -mtriple=x86_64-linux-gnu -global-isel   sub128.ll -o gisel.s
```

## Generated assembly (GISel, buggy)
```
sub_i128:
    movq    %rdi, %rax
    movq    %rsi, -8(%rsp)
    movq    %rdx, %rsi
    movq    -8(%rsp), %rdx
    subq    %rsi, %rax            # %rax = a.lo - b.lo   (CF = borrow)
    setb    %sil                  # %sil  = borrow (0 or 1)
    cmpb    $1, %sil              # CF = (sil < 1) = !borrow   <-- INVERTED
    sbbq    %rcx, %rdx            # rdx = a.hi - b.hi - (!borrow)
    setb    %cl
    retq
```

The correct lowering would either feed the original CF directly into
`sbbq` (no setb/cmp roundtrip at all) or rematerialize CF with
`addb $-1, %sil` / `bt $0, %sil`. Using `cmpb $1, %sil` inverts the bit.

## Runtime confirmation
Compile with `cc gisel_sub.o sub128_2.c` (and the SDAG object for
comparison). Inputs and outputs:

```
input a=0x1111111122222222_3333333344444444, b=0x0000000000000001_0000000000000001
SDAG : hi=1111111122222221 lo=3333333344444443   (correct: a - b)
GISel: hi=1111111122222220 lo=3333333344444443   (high off by 1)
```

```
input a = 1<<64, b = 1
SDAG : lo=ffffffffffffffff hi=0  (correct)
GISel: lo=ffffffffffffffff hi=1  (high off by 1)
```

## Smell
Sister bug of the previously filed `selectUAddSub` carry-in inversion
for ADD: same pattern of "materialize CF via setb then re-test with
`cmpb $1`", which negates the bit. Multi-word integer arithmetic at -O0
with GISel is broken for any subtraction whose low half does not borrow.
