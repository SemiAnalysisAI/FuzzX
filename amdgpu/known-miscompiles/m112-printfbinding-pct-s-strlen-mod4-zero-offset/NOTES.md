# m112: `AMDGPUPrintfRuntimeBinding` `%s` slot is one i32 too large when `strlen(s) % 4 == 0`

*Discovery method: code inspection.* Audit `a6b446e4ca831ee3c`.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUPrintfRuntimeBinding.cpp:218-220`
sizes the per-argument slot that `%s` will occupy in the printf record:

```cpp
if (shouldPrintAsStr(OpConvSpecifiers[ArgCount - 1], ArgType))
  ArgSize = alignTo(getAsConstantStr(Arg).size() + 1, 4);
```

`getAsConstantStr(Arg).size()` returns `strlen(s)` -- the trailing NUL
is *not* in the returned `StringRef`. So the slot the pass *promises*
the runtime is `alignTo(strlen + 1, 4)` bytes:

| strlen | promised slot |
| --- | --- |
| 3 (`"abc"`) | `alignTo(4, 4) = 4` |
| 4 (`"abcd"`) | `alignTo(5, 4) = 8` |
| 7 (`"abcdefg"`) | `alignTo(8, 4) = 8` |
| 8 (`"abcdefgh"`) | `alignTo(9, 4) = 12` |

But the *store* loop at `AMDGPUPrintfRuntimeBinding.cpp:357-389`
packs the same `S` (length `strlen`, no NUL) into i32 / i24 / i16 / i8
chunks, and the next-arg GEP at `:401-415` advances by
`getTypeAllocSize` of the actually-stored value:

```cpp
while (Offset && Offset.tell() < S.size()) {
  uint64_t ReadNow = std::min(ReadSize, S.size() - Offset.tell());
  ...                            // pushes one i8/i16/i24/i32 per chunk
}
...
unsigned ArgSize = TD->getTypeAllocSize(TheBtCast->getType());
BufferIdx = GetElementPtrInst::Create(I8Ty, BufferIdx,
                                      {ConstantInt::get(I32Ty, ArgSize)}, ...);
```

So the IR actually writes `roundUp(strlen, 4)` bytes for `%s` (one i32
per full 4-byte chunk, plus one zext'd-to-i32 short chunk for any
1..3-byte tail).

| strlen | promised | IR-written | delta |
| --- | --- | --- | --- |
| 3 | 4 | 4 (one i24 -> i32) | 0 |
| 4 | 8 | 4 (one i32) | **+4 promised** |
| 7 | 8 | 8 (i32 + i24 -> i32) | 0 |
| 8 | 12 | 8 (two i32) | **+4 promised** |
| 11 | 12 | 12 | 0 |
| 12 | 16 | 12 | **+4 promised** |

For any string with `strlen > 0 && strlen % 4 == 0`, the metadata slot
is exactly one i32 wider than the IR-emitted store sequence. The
next-arg GEP starts +4 *behind* where the runtime expects.

## Reproducer

`reduced.ll` (FuzzX format, `%s` argument is the literal `"abcd"`, the
`%d` argument is a volatile-loaded i32 from `%in`):

```llvm
@.fmt = private unnamed_addr addrspace(4) constant [6 x i8] c"%s %d\00"
@.str = private unnamed_addr addrspace(4) constant [5 x i8] c"abcd\00"

declare i32 @printf(ptr addrspace(4), ...)

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in,
                                       ptr addrspace(1) %out, i32 %n) {
  ; ... per-lane bounds check ...
  %x = load volatile i32, ptr addrspace(1) %in.ptr, align 4
  call i32 (ptr addrspace(4), ...) @printf(ptr addrspace(4) @.fmt,
                                           ptr addrspace(4) @.str,
                                           i32 %x)
  ...
}
```

Run:

```bash
amdgpu/build/llvm-fuzzer/bin/opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 \
    -passes=amdgpu-printf-runtime-binding -S \
    amdgpu/known-miscompiles/m112-.../reduced.ll
```

Key output:

```llvm
%printf_alloc_fn = call ptr addrspace(1) @__printf_alloc(i32 16)
...
%PrintBuffID    = getelementptr i8, ptr addrspace(1) %printf_alloc_fn, i32 0
store i32 1, ptr addrspace(1) %PrintBuffID, align 4               ; +0  id
%PrintBuffGep   = getelementptr i8, ptr addrspace(1) %printf_alloc_fn, i32 4
store i32 1684234849, ptr addrspace(1) %PrintBuffGep, align 4     ; +4  "abcd"
%PrintBuffNext  = getelementptr i8, ptr addrspace(1) %PrintBuffGep, i32 4
store i32 %x, ptr addrspace(1) %PrintBuffNext, align 4            ; +8  %d
...
!llvm.printf.fmts = !{!"1:2:8:4:%s %d"}                            ; %s=8, %d=4
```

The metadata `2:8:4` says arg1 (`%s`) occupies 8 bytes followed
immediately by arg2 (`%d`), so the runtime walks the record as:

| metadata offset | field | runtime reads |
| --- | --- | --- |
| +0 | id | `1` |
| +4..+12 | `%s` (8 B) | "abcd" + 4 trailing bytes |
| +12..+16 | `%d` (4 B) | **whatever was at +12** |

The IR stored `%x` at `+8`. The runtime fetches `%d` from `+12`,
where the kernel never wrote -- whatever bytes `__printf_alloc`
returned (typically the bump-allocator's stale memory; in practice
often zero on a fresh device buffer, but unspecified). The i32 value
the kernel *did* store (`%x` at `+8`) is absorbed into the `%s`
field's tail bytes and discarded by the runtime's string parser.

## Asm divergence

`llc -mcpu=gfx950 -O2` of the post-pass IR:

```
v_mov_b32_e32 v0, 16                ; __printf_alloc(16)
...
v_mov_b32_e32 v2, 1                 ; id at +0
v_mov_b32_e32 v3, 0x64636261        ; "abcd" packed at +4
v_mov_b32_e32 v4, s0                ; %x  at +8  (should be +12)
global_store_dwordx3 v[0:1], v[2:4], off
```

Only 12 bytes are stored, but the metadata advertised a 16-byte
record with `%d` at offset +12. The +12 word is never written by the
kernel.

## Per-strlen severity

`reduced.ll` covers `strlen == 4`. Other vulnerable strlens (`8, 12,
16, ...`) have the same +4 metadata offset; each *additional*
argument after `%s` is shifted further (`+4` always, because the bug
manifests once per `%s` argument).

A printf like `printf("%s%s%d", "abcd", "wxyz", x)` (two `%s` slots,
both strlen%4==0) misaligns `%d` by `+8`.

## Why this matters in the default pipeline

`AMDGPUPrintfRuntimeBinding` is `addModulePass`'d unconditionally
during the codegen pipeline at `-O0` and `-O2`
(`AMDGPUTargetMachine.cpp:1009, 2234`). Any HIP / OpenCL kernel that
calls `printf` with a `%s` whose constant-string argument has
`strlen % 4 == 0` (`"abcd"`, `"hello!\n\0"` -> 8 chars, `"%d\n"`-style
length-8 prefixes, ...) produces a buffer record the HSA printf
runtime cannot decode correctly: every subsequent format argument is
read from a +4-shifted slot.

## Suggested fix

Either:

1. Change the metadata to match the stores --

   ```cpp
   if (shouldPrintAsStr(OpConvSpecifiers[ArgCount - 1], ArgType))
     ArgSize = alignTo(getAsConstantStr(Arg).size(), 4);
   ```

   i.e. drop the `+ 1` (the runtime does not need a NUL terminator in
   the buffer record -- the size is carried in the metadata, and the
   length is implicit in the original format string anyway).

2. Or change the stores to match the metadata -- pad the final chunk
   out to the full `alignTo(strlen + 1, 4)` size by emitting one
   extra i32 zero store after the data, only when
   `strlen % 4 == 0`. Pseudocode at `:357-389`:

   ```cpp
   if ((S.size() & 3) == 0)
     WhatToStore.push_back(ConstantInt::get(Int32Ty, 0));
   ```

Either fix is local. Option (1) saves an i32 of buffer space per
mod-4 `%s`; option (2) preserves the NUL convention but at a
4-byte cost.

There is also a latent printf-runtime assumption to audit: it relies
on the bytes between `strlen` and `slot_end` being readable. With (1)
the slot tightly ends at `strlen`, so any runtime code that does
`for (i = 0; i < slot; ++i) if (buf[i] == 0) break;` keeps working.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`amdgpu/build/llvm-fuzzer/bin/opt`) | Reproduces (metadata `2:8:4`, IR stores 12 bytes). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Reproduces. |
| ROCm 7.2.3 (`amdgpu/build/rocm-7.2.3-extract/opt/rocm-7.2.3/lib/llvm/bin/opt`) | Reproduces -- bug is present in older LLVM too; not a recent regression. |

The FuzzX harness cannot drive an end-to-end runtime divergence for
this bug because the standard `hip_module_runner` does not enable the
HSA printf runtime (so printf is a no-op / silent fail) and
`__printf_alloc` is an external symbol that doesn't link in the
`-nogpulib` build path used by `run_ll_reproducer.sh`. The bug is at
the IR / metadata layout level, and the `opt -S` output above plus
the matching `llc` asm are the proof (same demonstration approach as
m085, m096, m097, m098).

## Why the fuzzer hasn't caught it

* The IR fuzzer does not emit `printf` calls -- printf lowering is a
  Clang front-end concern, and the FuzzX emitter produces pure IR.
* Even if it did, the FuzzX oracle compares device output buffers
  word-for-word; printf side effects are not part of the comparison
  surface.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  add `printf(fmt, str_const, ...)`-shaped emissions to the random
  emitter's call-builder, with `str_const` length chosen from
  `{1,2,3,4,5,6,7,8,...}` and an oracle that captures the printf
  buffer contents post-`__printf_alloc` (rather than the printed
  text) so the metadata / store divergence is observable without
  needing the HSA printf runtime.
