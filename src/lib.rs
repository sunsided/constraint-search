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

/// A constraint that removes values from variable domains.
///
/// A propagator reads the current domains, removes every value it can prove impossible and
/// reports whether it changed anything, so that propagation can run to a fixpoint.
pub trait Propagator<Domains> {
    /// Prunes `domains`; returns `Some(true)` if a domain shrank, `Some(false)` if nothing
    /// changed and `None` if the constraint can no longer be satisfied.
    fn propagate(&mut self, domains: &mut Domains) -> Option<bool>;
}
