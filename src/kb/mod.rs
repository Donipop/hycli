//! Knowledge base ledger — SQLite node/edge graph (WAL mode).
pub mod ops;

pub use ops::*;

#[cfg(test)]
mod tests;
