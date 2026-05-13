# ptx-fuzz design

## Goal

Find inputs that crash NVIDIA's `ptxas`. `ptxas` is closed-source with
no compile-time instrumentation, so we need binary-only coverage.

## Architecture

```
       ┌──────────────────────────┐
       │   afl-fuzz               │
       │  (manages corpus, picks  │
       │   inputs, schedules      │
       │   mutations)             │
       └─────────┬────────────────┘
                 │
                 │ dlopen()s
                 ▼
       ┌──────────────────────────┐
       │ libptx_fuzz_mutator.so   │
       │  afl_custom_post_process:│
       │    raw bytes → PTX text  │
       └─────────┬────────────────┘
                 │
                 │ writes PTX text to @@ input file
                 │ then fork()s qemu forkserver
                 ▼
       ┌──────────────────────────┐
       │  afl-qemu-trace ptxas @@ │
       │  (user-mode QEMU patches │
       │   TCG blocks to bump     │
       │   AFL coverage shmem)    │
       └─────────┬────────────────┘
                 │
                 │ ptxas crash → signal exit
                 ▼
       ┌──────────────────────────┐
       │  afl-fuzz saves input    │
       │  to output/.../crashes/  │
       └──────────────────────────┘
```

## Why AFL++ qemu_mode instead of libFuzzer + Valgrind?

We initially scaffolded a libFuzzer + Valgrind/callgrind pipeline.
Switched to AFL++ for these reasons:

| Factor                          | libFuzzer + Valgrind             | AFL++ qemu_mode                        |
| ------------------------------- | -------------------------------- | -------------------------------------- |
| Slowdown vs native ptxas        | 50–100×                          | 3–10×                                  |
| Coverage extraction complexity  | parse callgrind text per iter    | shmem, written directly by QEMU TCG    |
| Forkserver support              | no (fresh process each iter)     | yes (one ptxas init pays for many runs)|
| Standard for binary-only fuzz   | unusual                          | common                                 |
| Implementation complexity       | custom Valgrind tool needed eventually | drop in afl-qemu-trace             |

The forkserver speedup alone is significant for ptxas, which spends a
non-trivial chunk of its startup loading libraries and parsing CLI
flags.

## Why a custom mutator and not just `afl-fuzz -- ptxas @@`?

We want AFL to mutate compact representations (raw bytes), not PTX text
directly. If AFL bit-flipped PTX text, it'd hit syntax errors on
nearly every mutation — most random edits to `.reg .u32 r0;` produce
unparseable garbage that ptxas rejects at the lexer. The byte-level
input gives the mutator a smooth space to explore, and `generate_ptx`
turns each byte string into something that at least makes it past the
lexer.

The natural AFL++ hook for this is **`afl_custom_post_process`**:

  - AFL's corpus and mutators see raw bytes.
  - Just before AFL writes the input file for the target, our hook
    transforms those bytes into PTX text.
  - Crashes are saved as the raw bytes; `ptx-fuzz-repro` re-applies the
    transform to recover what ptxas actually saw.

The alternative (`afl_custom_fuzz`) replaces AFL's mutators — we'd
have to reimplement bit flips, splices, etc. ourselves. Not worth it.

## Why not `afl_custom_fuzz` + a structured grammar?

A grammar-aware generator would produce more *syntactically* valid PTX
and exercise deeper code paths faster. That's the right next step once
the pipeline is solid. v0 stays with raw-bytes-passthrough because:

1. It validates the AFL++ plumbing without confounding from a
   complicated generator.
2. Even the byte passthrough finds the seeded `@!` crash in our local
   `fake-ptxas` quickly, so the coverage feedback loop is clearly
   working.
3. A grammar is a meaningful chunk of work that benefits from being
   built once we know what shapes of PTX are interesting to `ptxas`.

## What's out of scope (for now)

- **Differential testing against the GPU.** Mentioned as a follow-on
  in the original request; only crash hunting in v0.
- **Persistent mode.** AFL++ supports `AFL_QEMU_PERSISTENT_ADDR` to
  loop inside ptxas without re-forking. Big win, but needs to know
  ptxas's `main` address (or a stable function early in startup), so
  skipped until we measure baseline throughput first.
- **Multi-core fuzzing.** AFL++ does this trivially with
  `afl-fuzz -M main` / `-S secondary-N`. Wait until single-core is
  working.
- **Grammar-aware generator.** See above.
- **Sandboxing.** ptxas is mostly-trusted, but running attacker-shaped
  inputs through it in a loop deserves a thought-out story
  eventually.

## Known issues

- AFL++ qemu_mode only really works on Linux, so this entire pipeline
  is Linux-only end-to-end. macOS can do `cargo build` / `cargo test`
  and exercise `fake-ptxas` manually, but the actual fuzz loop has to
  run on the Linux box.
- The seeded-crash patterns in `fake-ptxas` are deliberately easy
  (specifically, two adjacent printable-ASCII bytes). A real ptxas
  crash will require AFL to discover deeper structural patterns, which
  is where the grammar work above starts to matter.
