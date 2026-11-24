//! Deterministic work samples of the `mover-pairs` operation (#1149) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/mover_pairs.rs"]
mod mover_pairs;

fn main() -> Result<(), sim_foundation::physics::PhysicsError> {
    let samples = mover_pairs::SCALE_POINTS
        .iter()
        .map(|&(walkers, sleepers)| mover_pairs::sample(walkers, sleepers))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"mover-pairs\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
