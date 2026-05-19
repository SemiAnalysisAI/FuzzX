# m083-orphan-param-ld

Found while continuing the CUDA 13.2.78 sweep after adding randomized
helper-call islands, nested-helper-island variants, denser branch tables, and
signed `redux.sync` coverage:

```text
divergences/active-20260519-181952-signed-redux/div-1779214814-18b10a6d6832d49f
```

The saved fuzzer program had the full rich-helper-call prologue plus random
body instructions. Reduction shrank the trigger to a kernel that declares a
local `.param` and reads from it with `ld.param`, with no preceding `st.param`
or `call` that uses the `.param` as an argument or return:

```ptx
.version 8.8
.target sm_103
.address_size 64

.visible .entry k()
{
    .reg .b32 %r<1>;
    .param .b32 x;
    ld.param.u32 %r0, [x];
    ret;
}
```

`ptxas -arch=sm_103` segfaults at every optimization level on this 11-line
input. Adding a preceding `st.param.b32 [x], %r1;` does not make the crash go
away. Wrapping the `.param`/`ld.param` in a proper `st.param ... call ...
ld.param` sequence (so `x` is actually used as a call argument or return
value) compiles cleanly. So the trigger is reading a `.param` declaration that
is never used as a call argument or return value — likely undefined behavior
per the PTX spec, but a crash is still wrong; ptxas should diagnose or accept,
not segfault.

Reproduce:

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas
$PTXAS -arch=sm_103 -O3 known-miscompiles/m083-orphan-param-ld/reduced.ptx -o /tmp/_t.cubin
```

Observed result:

```text
Segmentation fault (core dumped)
```

This reproduced on 2026-05-19 with both:

* CUDA Toolkit 13.2 Update 1 ptxas:
  `release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`
* CUDA Toolkit 13.0 ptxas:
  `release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`

The fuzzer-generated `original.ptx` shares the same fundamental trigger but
buried inside the rich-helper-call prologue and several blocks of random body.
Its `.param` declarations are all actually used in `call.uni` sequences, yet
the program still crashes ptxas at `-O3` — suggesting the bug is more general
than just the orphan-read minimum, or that some interaction in the body
re-creates an equivalent shape. The 11-line reduced PTX is the smallest
self-contained crash we have for this family.

The current fuzzer naturally emits the `.param` declarations only when
`emit_rich_helper_calls` is true, and always pairs them with proper `st.param
... call ... ld.param` sequences. The fuzzer still rediscovers the crash via
the more complex original-style trigger; using `DIV_DISABLE_RICH_HELPER_CALLS=1`
removes the `.param` declarations entirely and avoids the family.
