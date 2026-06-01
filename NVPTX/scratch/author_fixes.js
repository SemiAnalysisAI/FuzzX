export const meta = {
  name: 'nvptx-author-fixes',
  description: 'Author a minimal correct fix (exact edits) for each remaining NVPTX bug',
  phases: [{ title: 'Author' }],
}

const BUGS_DIR = '/Users/justinlebar/code/FuzzX/NVPTX/bugs'
const SRC = '/Users/justinlebar/code/llvm2/llvm/lib/Target/NVPTX'   // EDIT TARGET (this is the build tree)
const INC = '/Users/justinlebar/code/llvm2/llvm/include/llvm/IR'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'           // current (buggy) build
const OPT = '/Users/justinlebar/code/llvm2/build/bin/opt'

const BUGS = Array.isArray(args) ? args : []

const SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    bug: { type: 'string' },
    fixable: { type: 'boolean' },
    needs_td_rebuild: { type: 'boolean' },
    files: { type: 'array', items: {
      type: 'object', additionalProperties: false,
      properties: {
        path: { type: 'string' },
        edits: { type: 'array', items: {
          type: 'object', additionalProperties: false,
          properties: { old_string: { type: 'string' }, new_string: { type: 'string' } },
          required: ['old_string', 'new_string'],
        } },
      },
      required: ['path', 'edits'],
    } },
    rationale: { type: 'string' },
    general_case_safe: { type: 'string' },
    confidence: { type: 'number' },
  },
  required: ['bug', 'fixable', 'needs_td_rebuild', 'files', 'rationale', 'general_case_safe', 'confidence'],
}

function prompt(bug) {
  return `You are writing the real upstream fix for ONE confirmed LLVM NVPTX backend bug. Produce a minimal, correct source edit.

Bug folder: ${BUGS_DIR}/${bug}/
- Read ${BUGS_DIR}/${bug}/NOTES.md (mechanism + root cause + often a suggested fix) and repro.ll and cmd.sh.

EDIT TARGET: the source under ${SRC} (and ${INC}/IntrinsicsNVVM.td for intrinsic defs). This IS the build tree; paths in your output MUST be absolute paths under /Users/justinlebar/code/llvm2/llvm/. Read the actual current file content there.

TOOLS:
- Current (buggy) compiler: ${LLC} and ${OPT}. Run the repro to see the current wrong/crashing behavior.
- ORACLE for constant/global-layout bugs (AsmPrinter): run the SAME repro with \`${LLC.replace('/llc','/llc')} -mtriple=x86_64 ...\` (x86) — the bytes/layout x86 emits are the CORRECT answer; your NVPTX fix should make NVPTX match that layout/value.
- For arch-validity ("invalid PTX on wrong target") bugs: grep the .td for how a SIBLING instruction is guarded (e.g. \`Requires<[hasSM<...>, hasPTX<...>]>\`, \`let Predicates = [...]\`, or \`has<Feature>()\` in NVPTXSubtarget.h) and use the SAME predicate. The NOTES usually state the exact required sm/PTX version.

DESIGN RULES:
- MINIMAL change that fixes the repro WITHOUT breaking the common/general case. Explicitly reason about the general case in 'general_case_safe' (e.g. "byte-or-larger types still take the existing path", "the predicate matches the instruction's real min arch so valid uses still select").
- Match surrounding LLVM code style.
- For a crash: make valid IR compile (or fall back to the correct existing path / a clean expansion); do NOT merely silence with a wrong result.
- For a miscompile: produce the correctly-rounded / correctly-signed / correctly-laid-out result; verify against the NOTES' "Expected" and/or x86.
- For arch-validity / wrong-qualifier: add the missing predicate / fix the qualifier so valid targets still work and invalid ones get a clean 'Cannot select' (NOT bad PTX). Prefer guarding the Pattern/instruction predicate.
- If the cleanest fix is in a C++ getMachineNode path that bypasses a tablegen Requires<>, add an explicit subtarget check there.

OUTPUT (structured): exact edits as old_string/new_string pairs.
- old_string MUST be copied VERBATIM from the current file content (exact whitespace/indentation) and be UNIQUE in that file. If the change site is not unique, include enough surrounding lines to make it unique.
- Keep each edit tight. Multiple edits/files allowed.
- Set needs_td_rebuild=true if any edited file is a .td (or include/.../IntrinsicsNVVM.td).
- If after analysis you believe there is no clean minimal fix (e.g. needs a large redesign), set fixable=false and explain in rationale (still give your best-effort edits if any).

Do the analysis, then return the structured output. Do NOT attempt to build (you cannot); reason carefully instead.`
}

phase('Author')

const results = await parallel(BUGS.map(b => () =>
  agent(prompt(b), { label: b.slice(0, 28), phase: 'Author', schema: SCHEMA })
    .then(r => ({ ...r, bug: b }))
    .catch(e => ({ bug: b, fixable: false, files: [], rationale: 'agent error: ' + String(e), needs_td_rebuild: false, general_case_safe: '', confidence: 0 }))
))

const fixable = results.filter(r => r.fixable && r.files && r.files.length)
log(`Authored: ${fixable.length}/${BUGS.length} fixable with edits`)
return { results }
