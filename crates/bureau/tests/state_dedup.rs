//! Dedup-marker semantics (DESIGN.md section 7, layer 5): `Rejected`
//! is terminal — no later `mark_seen` overwrites the rejection audit
//! trail — while weaker markers still flip: `Proposed` becomes
//! `Rejected` when review closes the PR.

use bureau::state::{Disposition, Store};

/// Writes each marker in order, returning the stored disposition.
fn settle(store: &Store, hash: &str, writes: &[Disposition]) -> Option<Disposition> {
    for disposition in writes {
        store.mark_seen(hash, *disposition).expect("mark_seen");
    }
    store.disposition(hash).expect("disposition")
}

#[test]
fn an_unmarked_hash_has_no_disposition() {
    let store = Store::open_in_memory().expect("open");
    let found = store.disposition("hash-1").expect("disposition");
    let seen = store.seen("hash-1").expect("seen");
    assert_eq!((seen, found), (false, None));
}

#[test]
fn a_rejected_marker_survives_later_writes() {
    let store = Store::open_in_memory().expect("open");
    let rejected = Some(Disposition::Rejected);
    let after_proposed = settle(
        &store,
        "hash-1",
        &[Disposition::Rejected, Disposition::Proposed],
    );
    let after_no_change = settle(&store, "hash-1", &[Disposition::NoChange]);
    let seen = store.seen("hash-1").expect("seen");
    let outcome = (after_proposed, after_no_change, seen);
    assert_eq!(outcome, (rejected, rejected, true), "rejection is terminal");
}

#[test]
fn a_proposed_marker_flips_to_rejected() {
    let store = Store::open_in_memory().expect("open");
    let stored = settle(
        &store,
        "hash-1",
        &[Disposition::Proposed, Disposition::Rejected],
    );
    assert_eq!(stored, Some(Disposition::Rejected), "review closed the PR");
}

#[test]
fn weaker_markers_overwrite_each_other() {
    let store = Store::open_in_memory().expect("open");
    let no_change = settle(
        &store,
        "hash-1",
        &[Disposition::Proposed, Disposition::NoChange],
    );
    let proposed = settle(
        &store,
        "hash-2",
        &[Disposition::NoChange, Disposition::Proposed],
    );
    let expected = (Some(Disposition::NoChange), Some(Disposition::Proposed));
    assert_eq!((no_change, proposed), expected);
}
