use super::super::context::measured_cost;
use crate::adapters::Usage;

#[test]
fn only_valid_adapter_cost_is_counted() {
    let cases = [
        (None, 0.0_f64),
        (Some(f64::NAN), 0.0),
        (Some(-1.0), 0.0),
        (Some(0.42), 0.42),
        (Some(1_000.0), 1_000.0),
    ];
    for (cost_usd, want) in cases {
        let usage = Usage {
            cost_usd,
            ..Usage::default()
        };
        let got = measured_cost(&usage);
        assert_eq!(got.to_bits(), want.to_bits(), "measured {cost_usd:?}");
    }
}
