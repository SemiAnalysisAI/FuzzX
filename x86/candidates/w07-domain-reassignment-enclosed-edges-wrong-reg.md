# X86DomainReassignment: buildClosure records wrong vreg in EnclosedEdges

File: llvm/lib/Target/X86/X86DomainReassignment.cpp:546-557

## Reasoning

In `buildClosure`:

```cpp
void X86DomainReassignmentImpl::buildClosure(Closure &C, Register Reg) {
  SmallVector<Register, 4> Worklist;
  ...
  while (!Worklist.empty()) {
    Register CurReg = Worklist.pop_back_val();

    // Register already in this closure.
    if (!C.insertEdge(CurReg))
      continue;
    EnclosedEdges[Reg] = C.getID();     // <-- uses outer `Reg`, not `CurReg`
    ...
```

The line `EnclosedEdges[Reg] = C.getID();` overwrites the same `Reg` (the initial seed register passed to `buildClosure`) on every iteration of the while loop. It should be `EnclosedEdges[CurReg] = C.getID();`. The bug means:

1. Most members of a closure are never recorded in `EnclosedEdges`, so a later outer loop in `runOnMachineFunction` (line 796-819) iterating over all virtual registers will re-enter `visitRegister` for them. `visitRegister` (line 422) only checks `EnclosedEdges.find(Reg)` to detect cross-closure aliasing. Because the entries are missing, two distinct seed walks can both walk through the same vregs and (a) build duplicate/overlapping closures, (b) skip the `C.setAllIllegal()` cross-closure guard that should fire when the same edge belongs to two closures.

The first closure ends up correctly converted; the second closure, sharing edges, has `reassign()` called on it and tries to `setRegClass` on a register that has already been reassigned to VK*. `getDstRC` (line 59) then asserts because the input is no longer a GR class, or it silently rewrites the second closure’s instructions using converters keyed on the (already‑replaced) original opcodes — leading to verifier failures or wrong code if no assertion fires (release build).

## Reproducer sketch (IR, AVX-512 BWI required)

A function with two disconnected mask-eligible chains that share an intermediate vreg via a PHI or COPY:

```llvm
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
  ; Two chains both reaching the same PHI form overlapping closures.
  %n = xor i64 %m, %a
  %o = and i64 %n, %b
  ret i64 %o
}
attributes #0 = { "target-features"="+avx512bw,+avx512dq" }
```

## Expected wrong outcome

Either (a) an assertion in `getDstRC` (`"add register class"`) firing in `+Asserts` builds for the second closure, or (b) in release: a register-class mismatch where `setRegClass` is called twice resulting in machine verifier complaints / mis-encoded KMOV* using already-K registers. Easy to spot with `llc -O2 -mattr=+avx512bw,+avx512dq -verify-machineinstrs`.
