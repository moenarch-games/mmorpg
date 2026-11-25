//! Deterministic work samples of the `flock-step` operation (#1019) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/flock_step.rs"]
mod flock_step;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = flock_step::SCALE_POINTS
        .iter()
        .map(|&(strays, idle)| flock_step::sample(strays, idle))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"flock-step\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
