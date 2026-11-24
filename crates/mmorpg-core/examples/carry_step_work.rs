//! Deterministic work samples of the `carry-step` operation (#1077) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/carry_step.rs"]
mod carry_step;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = carry_step::SCALE_POINTS
        .iter()
        .map(|&(carriers, idle)| carry_step::sample(carriers, idle))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"carry-step\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
