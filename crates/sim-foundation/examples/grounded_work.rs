//! Deterministic work samples of the `grounded-reads` operation (#1148) in
//! `.performance/contract.json`, printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the envelope.
//! Counters only; nothing is timed.
#[path = "../tests/support/grounded_reads.rs"]
mod grounded_reads;

fn main() -> Result<(), sim_foundation::physics::PhysicsError> {
    let samples = grounded_reads::SCALE_POINTS
        .iter()
        .map(|&(units, ticks)| grounded_reads::sample(units, ticks))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"grounded-reads\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
