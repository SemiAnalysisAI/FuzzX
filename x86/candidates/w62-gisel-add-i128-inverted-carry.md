# GISel `add i128` x86_64 — runtime witness for the inverted-carry bug from w14

## Summary

This is an additional runtime witness for the GISel inverted-carry bug
already reported in **w14-uadde-cmp-inverted-carry.md** (and the symmetric
`sub i128` borrow case in bug 110). The original w14 ticket demonstrates the
issue on i386. This candidate confirms the same `setb / cmpb $1 / adcq`
sequence fires on x86_64 for the IR-level `add i128 %a, %b` (and `sub i128`
mirror-image), giving a directly-observable runtime miscompile of an
unmodified `__int128` add.

It complements bug 110 (sub128 borrow inversion already on disk) with the
symmetric ADD direction.

## IR (`repro.ll`)

```llvm
define i128 @add_i128(i128 %a, i128 %b) {
  %r = add i128 %a, %b
  ret i128 %r
}
```

## Runner (`runner.c`)

```c
#include <stdio.h>
#include <stdint.h>
__int128 add_i128(__int128, __int128);
int main(void){
    /* a = 2^64, b = 1.  add -> hi=1 lo=1 (no carry from low half). */
    __int128 a = ((__int128)1) << 64;
    __int128 b = 1;
    __int128 r = add_i128(a, b);
    uint64_t lo = (uint64_t)r, hi = (uint64_t)(r >> 64);
    printf("add128(1<<64, 1) hi=0x%016lx lo=0x%016lx (expected hi=1 lo=1)\n",
           hi, lo);
    if (hi == 1 && lo == 1) { puts("OK"); return 0; }
    puts("FAIL — GISel add-i128 carry inverted"); return 1;
}
```

## Asm (-O0 -global-isel, x86_64)

```
add_i128:
	movq	%rdi, %rax
	movq	%rsi, -8(%rsp)
	movq	%rdx, %rsi
	movq	-8(%rsp), %rdx
	addq	%rsi, %rax       # %rax = a.lo + b.lo, CF = real carry
	setb	%sil             # %sil = CF
	cmpb	$1, %sil         # CF = 1 iff %sil < 1 iff %sil == 0  (INVERTED)
	adcq	%rcx, %rdx       # %rdx = a.hi + b.hi + (1 - real_carry)  <-- WRONG
	setb	%cl
	retq
```

## Reproduction

```bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
$LLC -O0 -mtriple=x86_64-linux-gnu -global-isel -filetype=obj repro.ll -o repro.o
cc -O0 runner.c repro.o -o runner
./runner
# Observed: add128(1<<64, 1) hi=0x0000000000000002 lo=0x0000000000000001
# Expected: hi=1 lo=1
```

| Variant       | hi                  | lo                  |
|---------------|---------------------|---------------------|
| -O0 (SDAG)    | 0x0…01              | 0x0…01              |
| -O2 (SDAG)    | 0x0…01              | 0x0…01              |
| -O0 -gisel    | 0x0…02 (wrong)      | 0x0…01              |

## Also-affected patterns (same root cause)

All of the following also produce wrong i128 results under GISel x86_64,
because they all generate the same `setb/cmpb $1/adcq|sbbq` chain:

| IR pattern          | Input                | Expected hi | GISel hi | Notes |
|---------------------|----------------------|-------------|----------|-------|
| `add i128 %a, %b`   | (1<<64, 1)           | 1           | 2        | phantom carry |
| `add i128 %a, 0`    | (1<<64, 0)           | 1           | 2        | constant 0 still emits the carry chain |
| `sub i128 %a, %b`   | (1<<64, 1)           | 0           | 1        | bug 110 (sub128 borrow) |
| `sub i128 %a, 0`    | (1<<64, 0)           | 1           | 2        | phantom borrow added to hi |
| `sub i128 %a, 1`    | (0, 1)               | -1=0xff..ff | 0        | real borrow lost |
| `extractelement (add128)` -> hi | (MAX, 1) -> hi | 1 | 0 | lost carry visible at hi alone |

The pattern: regardless of whether there is a real carry/borrow from the
low half, the GISel-emitted high-half `adcq`/`sbbq` reads the *complement*
of the low-half flag, so the high word is always off by one in the wrong
direction.

## Relation to prior tickets

- **w14-uadde-cmp-inverted-carry.md** — same root cause, demonstrated on
  i386; cites the bad MIR check
  `llvm/test/CodeGen/X86/GlobalISel/select-add-x32.mir` as the reason this
  pattern slipped past the test suite. This candidate confirms the bug also
  manifests on x86_64 for plain `__int128` arithmetic.
- **bug 110 / `gisel-usube-inverted-borrow-sub128`** — already filed for the
  `sub i128` half of the same defect. This candidate is the `add i128`
  symmetric witness.

## Suggested fix

In `llvm/lib/Target/X86/GISel/X86InstructionSelector.cpp` `selectUAddSub`
carry-in materialization, replace the `CMP8ri %sil, 1` with either
`ADD8ri %sil, $-1` or `NEG %sil` (either sets CF iff the byte is non-zero,
which matches the SDAG canonical form and gives `ADC` / `SBB` the correct CF
in either direction).

Files (read for context):
- `/home/orenamd@semianalysis.com/FuzzX/x86/candidates/w14-uadde-cmp-inverted-carry.md`
- `/home/orenamd@semianalysis.com/FuzzX/x86/bugs/110-gisel-usube-inverted-borrow-sub128/`
