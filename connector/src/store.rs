//! The connector's state on disk: the journal of `trommi-io` (core/io/src/journal.rs), which the Linux app's
//! executor writes its device space with as well.
pub use trommi_io::journal::*;

impl From<StoreError> for crate::error::Fault {
    fn from(error: StoreError) -> Self {
        let code = match error {
            StoreError::Locked => "state-locked",
            StoreError::Damaged(_) => "state-damaged",
            StoreError::Io(_) | StoreError::Poisoned => "storage",
        };
        crate::error::Fault::new(code, error.to_string())
    }
}
