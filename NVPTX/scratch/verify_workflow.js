export const meta = {
  name: 'nvptx-verify-miscompiles',
  description: 'Empirically test + adversarially verify NVPTX miscompile candidates with the built llc',
  phases: [{ title: 'Verify' }, { title: 'Refute' }],
}

const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'
const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const SCRATCH = '/Users/justinlebar/code/FuzzX/NVPTX/scratch'

// args is an array of candidate objects: {id, region, title, file, lines, kind, mechanism, trigger, ir, llc_cmd, confidence}
const CANDIDATES = Array.isArray(args) ? args : (args && args.candidates) || []

const VERIFY_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    id: { type: 'string' },
    real: { type: 'boolean' },
    kind: { type: 'string', enum: ['miscompile', 'segfault', 'assertion', 'not-a-bug'] },
    confidence: { type: 'number' },
    reasoning: { type: 'string' },
    repro_ir: { type: 'string' },
    llc_cmd: { type: 'string' },
    actual_output: { type: 'string' },
    expected: { type: 'string' },
  },
  required: ['id', 'real', 'kind', 'confidence', 'reasoning', 'repro_ir', 'llc_cmd', 'actual_output', 'expected'],
}

const REFUTE_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    id: { type: 'string' },
    verdict: { type: 'string', enum: ['confirmed', 'refuted', 'uncertain'] },
    confidence: { type: 'number' },
    reasoning: { type: 'string' },
  },
  required: ['id', 'verdict', 'confidence', 'reasoning'],
}

function verifyPrompt(c) {
  return `You are empirically verifying a SUSPECTED bug in the LLVM NVPTX backend. Be rigorous and skeptical: most candidates turn out NOT to be bugs.

Candidate id: ${c.id}
Title: ${c.title}
Location: ${c.file} ${c.lines || ''}
Kind claimed: ${c.kind}
Mechanism claimed:
${c.mechanism}
Trigger: ${c.trigger}
Proposed IR:
${c.ir}
Proposed llc cmd: ${c.llc_cmd}

TOOLS:
- Built compiler with NVPTX enabled: ${LLC}
- NVPTX backend source: ${SRC}
- Scratch dir for temp files: ${SCRATCH} (write .ll files here, name them ${c.id}-*.ll)

WHAT TO DO:
1. Read the cited source lines to confirm the mechanism is real (not a misreading).
2. Write a minimal, VALID, non-UB LLVM IR reproducer to ${SCRATCH}/${c.id}.ll. Use 'poison' not 'undef' unless undef is the point. Make sure it is something the backend actually accepts (no "cannot select"); if it errors for an unrelated reason, adjust until the relevant path is exercised.
3. Run: ${LLC} <flags> ${SCRATCH}/${c.id}.ll -o - and inspect the PTX.
   - For a MISCOMPILE: show that the emitted PTX computes a different value than the input IR semantics for some concrete, well-defined input. Pin down the exact discrepancy (which instruction, what it computes vs what it should). If you cannot exhibit a concrete defined input where results differ, it is NOT a miscompile.
   - For a SEGFAULT/ASSERTION: show llc actually crashes/asserts on valid IR (capture the message and the stack). A crash only on invalid IR does NOT count.
4. Be adversarial against your own claim. Common reasons a candidate is NOT a bug: the IR-level semantics already permit the behavior (UB, poison, freeze); the value is masked/normalized elsewhere; the suspicious code is dead or guarded; the PTX is actually correct on closer reading; the input needed is UB.

STANDARDS:
- "miscompile" only if you have a concrete defined input with a demonstrable wrong result (or a clearly wrong emitted instruction whose semantics you can pin down).
- "segfault"/"assertion" only if you reproduced the crash with the built llc on valid IR (paste the real output).
- Otherwise "not-a-bug".

Fill the structured output. Put the EXACT minimal IR in repro_ir, the exact llc command (with the path) in llc_cmd, the relevant snippet of real llc output in actual_output, and what a correct compiler should produce in expected. Set real=true only for miscompile/segfault/assertion that you actually substantiated.`
}

function refutePrompt(v, c) {
  return `An independent verifier claims the following is a REAL bug in the LLVM NVPTX backend. Your job is to REFUTE it. Default to skepticism: if the evidence is not airtight, lean toward 'refuted' or 'uncertain'.

Candidate id: ${v.id}
Title: ${c ? c.title : ''}
Claimed kind: ${v.kind}
Verifier reasoning: ${v.reasoning}
Repro IR:
${v.repro_ir}
llc cmd: ${v.llc_cmd}
Claimed actual output: ${v.actual_output}
Claimed expected: ${v.expected}

TOOLS: built llc at ${LLC}; source at ${SRC}; scratch ${SCRATCH}.

DO:
1. Re-run the exact command yourself (write the IR to ${SCRATCH}/${v.id}-refute.ll). Confirm the output matches what the verifier claimed.
2. Independently check whether the "wrong" output is actually wrong:
   - Recompute the IR semantics by hand for a concrete input.
   - Recompute what the emitted PTX does for that same input (mind PTX instruction semantics: e.g. shf.l/r.clamp clamps shift>=32 to 32; shl/shr by >=width is UB-ish in LLVM but defined-zero in PTX for some ops; cvt rounding; mul.wide sign; selp; setp predicate).
   - If they agree, it is REFUTED.
3. Check the IR is valid and non-UB. If the bug only manifests under UB/poison, REFUTE.
4. For crashes: confirm the crash reproduces and that the input IR is valid (verifier-clean). If it only crashes on invalid IR, REFUTE.

Output verdict: 'confirmed' (you reproduced it AND independently agree it is a real wrong-result/crash on valid IR), 'refuted' (it is not a bug, or only under UB, or does not reproduce), or 'uncertain'.`
}

phase('Verify')

const withIds = CANDIDATES.map((c, i) => ({ ...c, id: c.id || `c${String(i + 1).padStart(3, '0')}` }))

const results = await pipeline(
  withIds,
  c => agent(verifyPrompt(c), { label: `verify:${c.id}`, phase: 'Verify', schema: VERIFY_SCHEMA })
        .then(v => ({ ...v, _cand: c }))
        .catch(e => ({ id: c.id, real: false, kind: 'not-a-bug', confidence: 0, reasoning: 'verify error: ' + String(e), repro_ir: '', llc_cmd: '', actual_output: '', expected: '', _cand: c })),
  (v) => {
    if (!v || !v.real) return { ...(v || {}), refute: { verdict: 'refuted', confidence: 1, reasoning: 'verifier judged not-a-bug' } }
    return agent(refutePrompt(v, v._cand), { label: `refute:${v.id}`, phase: 'Refute', schema: REFUTE_SCHEMA })
      .then(r => ({ ...v, refute: r }))
      .catch(e => ({ ...v, refute: { verdict: 'uncertain', confidence: 0, reasoning: 'refute error: ' + String(e) } }))
  }
)

const clean = results.filter(Boolean).map(v => {
  const { _cand, ...rest } = v
  return { ...rest, region: _cand && _cand.region, title: _cand && _cand.title, file: _cand && _cand.file, lines: _cand && _cand.lines }
})
const confirmed = clean.filter(v => v.real && v.refute && v.refute.verdict === 'confirmed')
const uncertain = clean.filter(v => v.real && v.refute && v.refute.verdict === 'uncertain')
log(`Verify done: ${confirmed.length} confirmed, ${uncertain.length} uncertain, ${clean.length - confirmed.length - uncertain.length} refuted/not-a-bug`)
return { confirmed, uncertain, all: clean }
