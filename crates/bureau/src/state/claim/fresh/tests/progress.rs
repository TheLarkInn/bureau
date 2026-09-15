use super::{EventKind, Fixture, Operation, output};

fn append(fixture: &mut Fixture, size: usize) {
    fixture
        .owner
        .with_ownership(|| {
            fixture.log.append(
                EventKind::Output,
                output(Some("writer"), "combined", &"x".repeat(size)),
            )
        })
        .expect("output progresses during preparation");
}

fn streaming(operation: Operation, large_first: bool, steady_size: usize) -> (bool, usize) {
    let mut fixture = Fixture::new();
    let root = fixture.root.clone();
    let mut attempts = 0;
    let result = operation
        .run(&root, || {
            attempts += 1;
            if attempts <= 3 {
                let size = if large_first && attempts == 1 {
                    2 * 1024 * 1024
                } else {
                    steady_size
                };
                append(&mut fixture, size);
            }
        })
        .expect("admission progresses during output");
    fixture.close();
    (result, attempts)
}

#[test]
fn ongoing_output_does_not_starve_observation_or_fresh_claims() {
    for operation in [Operation::Observe, Operation::Claim] {
        assert_eq!(streaming(operation, false, 16), (true, 1));
    }
}

#[test]
fn large_catch_up_replays_outside_the_fence_then_admits_despite_more_output() {
    for operation in [Operation::Observe, Operation::Claim] {
        assert_eq!(streaming(operation, true, 128 * 1024), (true, 2));
    }
}

#[test]
fn sustained_output_bursts_do_not_force_repeated_preparation() {
    for operation in [Operation::Observe, Operation::Claim] {
        assert_eq!(streaming(operation, false, 128 * 1024), (true, 1));
    }
}
