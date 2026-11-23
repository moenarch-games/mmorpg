//! Deterministic work samples of the `creature-tick` operation (#1086) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/creature_tick.rs"]
mod creature_tick;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples = creature_tick::SCALE_POINTS
        .iter()
        .map(|&(idle, active)| creature_tick::sample(idle, active))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"creature-tick\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
