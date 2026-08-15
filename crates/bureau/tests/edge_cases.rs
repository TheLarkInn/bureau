//! Adversarial edge-case tests beyond the happy paths covered by
//! `tests/scrub.rs`, `tests/step_contract.rs`, `tests/state_store.rs`,
//! `tests/config_validate.rs`, `tests/run_log.rs`, and
//! `tests/process_contract.rs`. Split into `edge/` modules to stay under
//! the 300-line file cap.

#[path = "edge/config.rs"]
mod config;
#[path = "edge/contract.rs"]
mod contract;
#[path = "edge/process.rs"]
mod process;
#[path = "edge/runlog.rs"]
mod runlog;
#[path = "edge/scrub.rs"]
mod scrub;
#[path = "edge/state.rs"]
mod state;
#[path = "edge/testdir.rs"]
mod testdir;
