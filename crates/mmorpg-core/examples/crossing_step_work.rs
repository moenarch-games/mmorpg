//! Deterministic work samples of the `crossing-step` operation (#1019) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/crossing_step.rs"]
mod crossing_step;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = crossing_step::SCALE_POINTS
        .iter()
        .map(|&(crossings, idle)| crossing_step::sample(crossings, idle))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"crossing-step\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
