//! Holding back a partial character at a chunk boundary.
//!
//! A read boundary lands wherever the OS puts it, which for agent transcripts
//! is regularly inside a multi-byte character. Whoever turns those bytes into
//! text has to decode a whole character or none of it: decoding half of one
//! yields a replacement character, and the other half is then lost too.
//!
//! This is per stream, alongside the per-stream scrubbing holdback, because
//! stdout and stderr share one run-log sink — a tail carried across that sink
//! would be re-attached to whichever stream wrote next.

/// The length of the longest prefix of `bytes` that ends on a character
/// boundary.
///
/// Bytes that no later chunk can repair are included rather than held: a
/// genuinely invalid sequence decodes to a replacement character now, which
/// is the honest answer. Only an unfinished trailing character is withheld.
pub(super) const fn complete_prefix(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        Ok(_) | Err(_) => bytes.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::complete_prefix;

    /// `●` is E2 97 8F: cutting it anywhere must hold back the fragment, and
    /// reassembling the two halves must reproduce it exactly.
    #[test]
    fn withholds_an_unfinished_character_at_every_cut() {
        let full = "a●b".as_bytes();
        let rebuilt: Vec<String> = (0..=full.len())
            .map(|cut| {
                let head = complete_prefix(&full[..cut]);
                let carried = [&full[head..cut], &full[cut..]].concat();
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&full[..head]),
                    String::from_utf8_lossy(&carried)
                )
            })
            .collect();

        assert_eq!(rebuilt, vec!["a●b"; full.len() + 1]);
    }

    #[test]
    fn passes_through_bytes_no_later_chunk_can_repair() {
        let cases: [(&[u8], usize); 4] = [
            (b"ok", 2),
            (&[0xE2, 0x97], 0),       // an unfinished `●`: hold it back
            (&[0xFF, b'x'], 2),       // never valid: let it decode lossily now
            (&[b'a', 0xE2, 0x97], 1), // valid prefix, unfinished tail
        ];

        assert_eq!(
            cases.map(|(bytes, _)| complete_prefix(bytes)),
            cases.map(|(_, want)| want)
        );
    }
}
