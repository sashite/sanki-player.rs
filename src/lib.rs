#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), warn(missing_docs))]

pub mod api;
pub mod classify;
pub mod eval;
mod prng;
pub mod search;
pub mod tt;
pub mod values;

pub use api::{Choice, Context, EvalWeights, Limits, Occurrences, Strength};
pub use search::{assess, choose, MATE};

// The engine types a caller manipulates through this crate's API.
pub use sashite_sanki_engine::domain::half_move::Move;
pub use sashite_sanki_engine::domain::side::Side;
pub use sashite_sanki_engine::domain::variant::Variant;
pub use sashite_sanki_engine::position::Position;
