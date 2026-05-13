//! Public surface for single-node Reality Graph storage.
//!
//! The current implementation is intentionally kept behind modules so the
//! crate root stays small and the storage engine can be split further without
//! changing downstream imports.

mod storage;

pub use storage::*;
