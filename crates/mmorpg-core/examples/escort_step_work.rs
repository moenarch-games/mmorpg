//! Deterministic work samples of the `escort-step` operation (#1019) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/escort_step.rs"]
mod escort_step;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = escort_step::SCALE_POINTS
        .iter()
        .map(|&(escorts, idle)| escort_step::sample(escorts, idle))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"escort-step\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
