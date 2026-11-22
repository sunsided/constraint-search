//! # Constraint search
//!
//! Finite-domain constraint satisfaction: variables with small domains, constraints that
//! prune them, and search for the assignments that remain.
//!
//! ## Scope
//!
//! - Finite-domain variables with bitset domains ([`Domain`]) and a trail for undoing changes
//!   on backtrack ([`Domains`])
//! - Propagators run to a fixpoint ([`Propagator`]), including all-different with complete
//!   Hall-set reasoning ([`AllDifferent`]) and constraints given by their allowed tuples
//!   ([`Table`])
//! - Pluggable inference rules ordered by cost, for human-style solving, hints and difficulty
//!   grading ([`Rules`])
//! - Backtracking search with variable and value ordering heuristics under a
//!   [`search_core::Budget`], resumable so the work can be spread over time ([`Solver`],
//!   [`Solver::resume`])
//! - Solution counting with early stop, for checking that a puzzle has a unique solution
//!   ([`Solver::count`])
//! - Exact cover with dancing links ([`ExactCover`])
//!
//! ## Overview
//!
//! ```mermaid
//! flowchart LR
//!     model[Model: variables, propagators] --> solver[Solver: domains, trail, queue]
//!     solver -->|solve, count| search[backtracking + propagation]
//!     domains[Domains] --> rules[Rules: costed deductions]
//!     rules --> trace[Trace: steps, grade]
//! ```
//!
//! A [`Model`] holds the variables and constraints and is shared read-only; it is
//! `Send + Sync`. A [`Solver`] borrows it, owns the working state and reuses it across
//! solves, which suits generators that check uniqueness thousands of times. [`Rules`] work on
//! any [`Domains`] and explain a solve step by step instead.
//!
//! Domains are bitsets: `u16`, `u32`, `u64` and `u128` hold up to their bit width in values,
//! and [`BitSet`] holds more.
//!
//! ## Example
//!
//! Colour a map of four regions with three colours, so that neighbours differ:
//!
//! ```
//! use constraint_search::{AllDifferent, Domain, Model, Outcome, Solver, Stats};
//!
//! let mut model = Model::<u16>::new();
//! let [a, b, c, d] = [(); 4].map(|()| model.add_var(u16::range(3)));
//! for pair in [[a, b], [a, c], [b, c], [b, d], [c, d]] {
//!     model.add(AllDifferent::new(pair));
//! }
//!
//! let report = Solver::new(&model).solve(|_: &Stats| false);
//! let Outcome::Solved(colours) = report.outcome else { panic!("three colours suffice") };
//! assert_eq!(colours[0], colours[3]); // a and d must share a colour
//! ```
//!
//! The crate is `no_std` and needs `alloc`.
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

pub use all_different::{AllDifferent, MatchingScratch};
pub use domain::{BitSet, Domain, Values};
pub use exact_cover::ExactCover;
pub use model::Model;
pub use propagator::Propagator;
pub use rule::{HiddenSubset, Intersection, NakedSubset, Rule, Rules, Step, Trace};
pub use search::{Count, Outcome, Progress, Report, Solver, Stats, Status, ValueOrder, VarOrder};
pub use store::{Domains, Inconsistent, Var};
pub use table::Table;

/// Doctests for the code examples in README.md (not part of the API).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}
