//! # Constraint search
//!
//! Finite-domain constraint satisfaction: variables with small domains, constraints that
//! prune them, and search for the assignments that remain.
//!
//! ## Scope
//!
//! - Finite-domain variables with bitset domains and a trail for undoing changes on backtrack
//! - Propagators run to a fixpoint, including all-different with Hall-set reasoning
//! - Pluggable inference rules ordered by cost, for human-style solving and difficulty grading
//! - Backtracking search with variable and value ordering heuristics under a budget
//! - Solution counting with early stop, for checking that a puzzle has a unique solution
//! - Exact cover with dancing links
//!
//! Repository: <https://github.com/sunsided/constraint-search>

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod all_different;
mod domain;
mod exact_cover;
mod model;
mod propagator;
mod rule;
mod search;
mod store;
mod table;
#[cfg(test)]
mod testing;

pub use all_different::AllDifferent;
pub use domain::{BitSet, Domain, Values};
pub use exact_cover::ExactCover;
pub use model::Model;
pub use propagator::Propagator;
pub use rule::{HiddenSubset, Intersection, NakedSubset, Rule, Rules, Step, Trace};
pub use search::{Count, Outcome, Report, Solver, Stats, Status, ValueOrder, VarOrder};
pub use store::{Domains, Inconsistent, Var};
pub use table::Table;

/// Doctests for the code examples in README.md (not part of the API).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}
