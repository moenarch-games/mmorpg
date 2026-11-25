//! Deterministic work samples of the `supply-step` operation (#1019) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/supply_step.rs"]
mod supply_step;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = supply_step::SCALE_POINTS
        .iter()
        .map(|&(patrols, idle)| supply_step::sample(patrols, idle))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"supply-step\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
