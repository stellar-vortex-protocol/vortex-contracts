// #351: Module organization for intent_settlement contract
// Splits 5000-line lib.rs into cohesive modules without changing ABI

pub mod admin;
pub mod config;
pub mod allowlist;
pub mod bonds;
pub mod intents;
pub mod bids;
pub mod disputes;
pub mod backstop;
pub mod views;
pub mod storage;
pub mod types;
pub mod errors;
pub mod events;

// Re-export public types and enums for use in main lib.rs
pub use types::*;
pub use errors::*;
pub use events::*;
