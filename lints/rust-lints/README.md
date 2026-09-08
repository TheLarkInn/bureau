# rust-lints

Custom Rust checks, vendored from
[li-kai/rust-lints](https://github.com/li-kai/rust-lints) and run through
[Dylint](https://github.com/trailofbits/dylint).

## In Bureau

Run from the repository root:

```sh
bash scripts/lint.sh
```

Install prerequisites from the [script header](../../scripts/lint.sh).
Bureau treats lint findings as errors and bans `#[allow]`/`#[expect]` in
workspace code. Follow [AGENTS.md](../../AGENTS.md), not upstream's optional
suppression examples. Module dependencies belong in [dylint.toml](../../dylint.toml).

## Checks

Links describe options and examples.

| Check | Catches |
|---|---|
| [acyclic_modules](docs/acyclic-modules.md) | Cycles between sibling modules |
| [await_holding_unsendable](docs/await-holding-unsendable.md) | Guards or connections held across `.await` |
| [blocking_in_async](docs/blocking-in-async.md) | Blocking operations in async code |
| [debug_remnants](docs/debug-remnants.md) | Debug prints outside tests |
| [fallible_new](docs/fallible-new.md) | Constructors that can panic |
| [global_side_effect](docs/global-side-effect.md) | Uninjected time, randomness, environment reads, or misplaced logging setup |
| [map_init_then_insert](docs/map-init-then-insert.md) | Empty maps immediately filled with inserts |
| [module_dependencies](docs/module-dependencies.md) | Undeclared module dependencies |
| [needless_builder](src/lints/needless_builder.rs) | Builders for tiny structs |
| [panic_in_drop](docs/panic-in-drop.md) | Panics during cleanup |
| [proper_error_type](docs/proper-error-type.md) | Unstructured or incomplete public error types |
| [realtime_in_async_test](docs/realtime-in-async-test.md) | Real clocks in tests using Tokio time |
| [result_result](docs/result-result.md) | Nested `Result` types |
| [suggest_builder](docs/suggest-builder.md) | Large constructors without builders |
| [topological_ordering](docs/topological-ordering.md) | Items placed before their dependencies |
| [unbounded_channel](docs/unbounded-channel.md) | Channels without capacity limits |
| [unclear_exports](src/lints/unclear_exports.rs) | Glob or renamed imports |
| [unsafe_send_missing_drop](docs/unsafe-send-missing-drop.md) | Unsafe `Send` with thread-bound fields and no cleanup |
| [unstructured_log_fields](docs/unstructured-log-fields.md) | Log messages without structured fields |

For standalone installation, editor integration, and lint development, use
the [upstream instructions](https://github.com/li-kai/rust-lints).
See the [configuration guide](docs/recommended-lint-config.md) and
[Nix guide](docs/nix-packaging.md) for use outside Bureau.
