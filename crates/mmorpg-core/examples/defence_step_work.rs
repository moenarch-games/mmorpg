//! Deterministic work samples of the `defence-step` operation (#1019) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/defence_step.rs"]
mod defence_step;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = defence_step::SCALE_POINTS
        .iter()
        .map(|&(raiders, idle)| defence_step::sample(raiders, idle))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"defence-step\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
