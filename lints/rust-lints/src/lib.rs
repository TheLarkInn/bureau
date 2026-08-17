#![feature(rustc_private)]
#![cfg_attr(
    not(dylint_lib = "rust_lints"),
    allow(unknown_lints, reason = "enable dylint lint annotations")
)]

extern crate rustc_ast;
extern crate rustc_data_structures;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod config;
mod lints;
#[cfg(test)]
mod testing;

use rustc_lint::LintStore;
use rustc_session::Session;

dylint_linting::dylint_library!();

#[expect(
    clippy::no_mangle_with_rust_abi,
    reason = "dylint requires extern fn signature"
)]
#[expect(
    unsafe_code,
    reason = "dylint requires #[no_mangle] for plugin registration"
)]
#[unsafe(no_mangle)]
pub fn register_lints(sess: &Session, lint_store: &mut LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[
        lints::acyclic_modules::ACYCLIC_MODULES,
        lints::suggest_builder::SUGGEST_BUILDER,
        lints::needless_builder::NEEDLESS_BUILDER,
        lints::bon_builder_collector::BON_BUILDER_COLLECTOR,
        lints::proper_error_type::PROPER_ERROR_TYPE,
        lints::global_side_effect::GLOBAL_SIDE_EFFECT_TIME,
        lints::global_side_effect::GLOBAL_SIDE_EFFECT_RANDOMNESS,
        lints::global_side_effect::GLOBAL_SIDE_EFFECT_ENV,
        lints::global_side_effect::GLOBAL_SIDE_EFFECT_LOGGING_INIT,
        lints::map_init_then_insert::MAP_INIT_THEN_INSERT,
        lints::debug_remnants::DEBUG_REMNANTS,
        lints::fallible_new::FALLIBLE_NEW,
        lints::unbounded_channel::UNBOUNDED_CHANNEL,
        lints::blocking_in_async::BLOCKING_IN_ASYNC,
        lints::panic_in_drop::PANIC_IN_DROP,
        lints::unclear_exports::UNCLEAR_EXPORTS,
        lints::result_result::RESULT_RESULT,
        lints::realtime_in_async_test::REALTIME_IN_ASYNC_TEST,
        lints::unstructured_log_fields::UNSTRUCTURED_LOG_FIELDS,
        lints::module_dependencies::MODULE_DEPENDENCIES,
        lints::module_dependencies::MODULE_DEPENDENCIES_UNLISTED,
        lints::module_dependencies::MODULE_DEPENDENCIES_DEAD_EDGE,
        lints::topological_ordering::TOPOLOGICAL_ORDERING,
        lints::unsafe_send_missing_drop::UNSAFE_SEND_MISSING_DROP,
        lints::await_holding_unsendable::AWAIT_HOLDING_UNSENDABLE,
    ]);
    lint_store.register_pre_expansion_pass(|| {
        Box::new(lints::bon_builder_collector::BonBuilderCollector)
    });
    lint_store.register_late_pass(|_| Box::new(lints::suggest_builder::SuggestBuilder::new()));
    lint_store.register_late_pass(|_| Box::new(lints::needless_builder::NeedlessBuilder::new()));
    lint_store
        .register_late_pass(|_| Box::new(lints::proper_error_type::ProperErrorType::default()));
    lint_store.register_late_pass(|_| Box::new(lints::global_side_effect::GlobalSideEffect::new()));
    lint_store
        .register_late_pass(|_| Box::new(lints::map_init_then_insert::MapInitThenInsert::new()));
    lint_store.register_late_pass(|_| Box::new(lints::debug_remnants::DebugRemnants::new()));
    lint_store.register_late_pass(|_| Box::new(lints::fallible_new::FallibleNew::new()));
    lint_store.register_late_pass(|_| Box::new(lints::unbounded_channel::UnboundedChannel::new()));
    lint_store.register_late_pass(|_| Box::new(lints::blocking_in_async::BlockingInAsync::new()));
    lint_store.register_late_pass(|_| Box::new(lints::panic_in_drop::PanicInDrop::new()));
    lint_store.register_late_pass(|_| Box::new(lints::unclear_exports::UnclearExports::new()));
    lint_store.register_late_pass(|_| Box::new(lints::result_result::ResultResult::new()));
    lint_store.register_late_pass(|_| {
        Box::new(lints::unstructured_log_fields::UnstructuredLogFields::new())
    });
    lint_store.register_late_pass(|_| {
        Box::new(lints::realtime_in_async_test::RealtimeInAsyncTest::new())
    });
    lint_store
        .register_late_pass(|_| Box::new(lints::module_dependencies::ModuleDependencies::new()));
    lint_store.register_late_pass(|_| Box::new(lints::acyclic_modules::AcyclicModules::new()));
    lint_store
        .register_late_pass(|_| Box::new(lints::topological_ordering::TopologicalOrdering::new()));
    lint_store.register_late_pass(|_| {
        Box::new(lints::unsafe_send_missing_drop::UnsafeSendMissingDrop::new())
    });
    lint_store
        .register_late_pass(|_| Box::new(lints::await_holding_unsendable::AwaitHoldingUnsendable::new()));
}
