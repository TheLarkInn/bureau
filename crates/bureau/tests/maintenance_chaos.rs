//! Bounded, seed-driven offline regressions for repeated maintenance runs.

#[path = "maintenance_chaos/admission.rs"]
mod admission;
#[path = "maintenance_chaos/fixture.rs"]
mod fixture;

fn seed() -> u32 {
    match std::env::var("BUREAU_CHAOS_SEED") {
        Ok(value) => value.parse().expect("BUREAU_CHAOS_SEED must be a u32"),
        Err(std::env::VarError::NotPresent) => 0,
        Err(error) => panic!("reading BUREAU_CHAOS_SEED: {error}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn seeded_offline_invariants() {
    let seed = seed();
    let mut state = seed;
    for case in 0..8 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        eprintln!("BUREAU_CHAOS_SEED={seed} case={case} state={state}");
        admission::shared_assignment_limit(state).await;
    }
}
