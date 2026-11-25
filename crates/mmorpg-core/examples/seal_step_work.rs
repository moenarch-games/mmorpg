//! Deterministic work samples of the `seal-step` operation (#1019) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/seal_step.rs"]
mod seal_step;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = seal_step::SCALE_POINTS
        .iter()
        .map(|&(occupants, idle)| seal_step::sample(occupants, idle))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"seal-step\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
