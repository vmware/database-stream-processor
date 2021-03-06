//! Some basic operators.

pub mod adapter;
pub use adapter::{BinaryOperatorAdapter, UnaryOperatorAdapter};

pub(crate) mod inspect;
pub use inspect::Inspect;

pub(crate) mod apply;
pub use apply::Apply;

pub mod apply2;

mod plus;
pub use plus::{Minus, Plus};

mod z1;
pub use z1::{DelayedFeedback, DelayedNestedFeedback, Z1Nested, Z1};

mod generator;
pub use generator::{Generator, GeneratorNested};

mod consolidate;
mod integrate;
mod trace;

pub mod communication;

mod differentiate;

mod delta0;
pub use delta0::Delta0;

mod condition;
pub use condition::Condition;

mod index;
pub use index::Index;

mod join;
pub use join::Join;

mod join_range;

mod sum;
pub use sum::Sum;

mod distinct;
pub use distinct::Distinct;

mod filter_map;
pub use filter_map::FilterMap;

mod aggregate;
pub use aggregate::Aggregate;

mod window;

#[cfg(feature = "with-csv")]
mod csv;

mod neg;
pub use neg::UnaryMinus;

pub mod recursive;

#[cfg(feature = "with-csv")]
pub use self::csv::CsvSource;
