//! Dedup dispositions: how a content-hash marker was resolved when it
//! was written (DESIGN.md section 7, layer 5).

/// How a dedup marker was resolved when it was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// A PR was opened for this content.
    Proposed,
    /// The proposal was rejected (review closed it). Terminal: no later
    /// marker overwrites it, so the rejection audit trail survives.
    Rejected,
    /// The run engaged with this content but opened no PR — it found
    /// nothing to change (`NoWork`) or escalated (`Blocked`). The item
    /// is settled; only a content edit (a new hash) re-arms it.
    NoChange,
}

impl Disposition {
    /// The stored token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Rejected => "rejected",
            Self::NoChange => "no_change",
        }
    }

    /// Reads a stored token back; an unrecognized value (a corrupt or
    /// newer-written row) is `Error::UnknownDisposition`.
    pub(super) fn from_token(token: &str) -> Result<Self, super::Error> {
        match token {
            "proposed" => Ok(Self::Proposed),
            "rejected" => Ok(Self::Rejected),
            "no_change" => Ok(Self::NoChange),
            unknown => Err(super::Error::UnknownDisposition(unknown.to_owned())),
        }
    }
}
