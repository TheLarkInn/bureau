use super::Error;
use crate::state::{self, LeaseOwner};

fn failure(error: state::Error) -> Error {
    match error {
        state::Error::Io(error) => Error::Io(error),
        state::Error::LeaseLost(_) => Error::Ownership,
        error => Error::OwnershipCheck(error),
    }
}

pub(super) fn run<T>(
    owner: &LeaseOwner,
    operation: impl FnOnce() -> std::io::Result<T>,
) -> Result<T, Error> {
    owner.with_ownership(operation).map_err(failure)
}
