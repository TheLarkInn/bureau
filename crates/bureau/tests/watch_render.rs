//! `bureau watch` rendering and selection tests: the plain-text
//! snapshot and the pure helpers, over shared fixtures.

pub mod watch_support;

use bureau::watch;
use watch_support::{TestDir, write_active, write_finished_run, write_state_db};

#[test]
fn render_plain_formats_a_full_frame() {
    let dir = TestDir::new("render");
    write_active(dir.path(), "abc1234567890def");
    write_state_db(dir.path());
    write_finished_run(&dir.roots().runs, "demo-1000-aa", 1_000, 2.5);
    let frame = watch::load(&dir.roots(), None, 16, 253_000);
    let text = watch::render_plain(&frame).join("\n");
    let checks = [
        text.contains("bureau watch · config abc123456789 ·"),
        text.contains("demo-1000-aa")
            && text.contains("finished(success)")
            && text.contains("$2.50"),
        text.contains("$6.00 / $25.00") && text.contains("2 / 4"),
    ];
    assert_eq!(checks, [true, true, true], "{text}");
}

#[test]
fn text_helpers_format_compactly() {
    let ages: [(u64, &str); 4] = [
        (42_000, "42s"),
        (252_000, "4m12s"),
        (11_040_000, "3h04m"),
        (172_800_000, "2d"),
    ];
    for (ms, want) in ages {
        assert_eq!(watch::age_text(ms), want);
    }
    let scalars = (watch::clock_text(37_230_000), watch::money(2.5));
    assert_eq!(scalars, ("10:20:30Z".to_owned(), "$2.50".to_owned()));
}

#[test]
fn selection_survives_reordering() {
    let dir = TestDir::new("select");
    let runs = dir.roots().runs;
    write_finished_run(&runs, "demo-1000-aa", 1_000, 1.0);
    write_finished_run(&runs, "demo-2000-bb", 2_000, 1.0);
    let frame = watch::load(&dir.roots(), None, 16, 9_000);
    let found = watch::resolve_selection(&frame.runs, Some("demo-1000-aa"), 0);
    let gone = watch::resolve_selection(&frame.runs, Some("demo-9999-zz"), 9);
    let empty: Vec<watch::RunRow> = Vec::new();
    let outcome = (found, gone, watch::resolve_selection(&empty, None, 0));
    assert_eq!(outcome, (Some(1), Some(1), None));
}
