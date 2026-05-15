//! Generate many programs and verify ptxas accepts every one at both -O0 and
//! -O3. Generator bugs that emit syntactically-invalid PTX, or that produce
//! something one opt level rejects, show up here long before they pollute the
//! divergence inbox of the real fuzzer.

use fuzzx_exec::compile;
use fuzzx_execgen::{bytes_from_seed, generate_from_bytes, TARGET_ARCH};

const PROGRAM_BYTES: usize = 4096;

#[test]
fn ptxas_accepts_random_programs_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let mut failures: Vec<(u64, String, String)> = Vec::new();

    for seed in 0u64..50 {
        let bytes = bytes_from_seed(seed, PROGRAM_BYTES);
        let ptx = match generate_from_bytes(&bytes) {
            Ok(p) => p,
            Err(e) => {
                failures.push((seed, "generator".into(), format!("{e}")));
                continue;
            }
        };
        for opt in ["-O0", "-O3"] {
            if let Err(e) = compile(&ptx, &[arch_flag.as_str(), opt]) {
                failures.push((seed, opt.into(), format!("{e}\n--- ptx ---\n{ptx}")));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} ptxas rejections out of 50 seeds (showing first 3):\n{}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .map(|(seed, opt, msg)| format!("seed={seed} opt={opt}:\n{msg}"))
            .collect::<Vec<_>>()
            .join("\n\n=====\n\n"),
    );
}
