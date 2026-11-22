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

mod domain;
mod store;

pub use domain::{BitSet, Domain, Values};
pub use store::{Domains, Inconsistent, Var};

/// A constraint that removes values from variable domains.
///
/// A propagator reads the current domains, removes every value it can prove impossible and
/// reports whether it changed anything, so that propagation can run to a fixpoint.
pub trait Propagator<Domains> {
    /// Prunes `domains`; returns `Some(true)` if a domain shrank, `Some(false)` if nothing
    /// changed and `None` if the constraint can no longer be satisfied.
    fn propagate(&mut self, domains: &mut Domains) -> Option<bool>;
}

/// Doctests for the code examples in README.md (not part of the API).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}

/// Unit tests for the `Propagator` trait, through a minimal implementation.
#[cfg(test)]
mod tests {
    use super::Propagator;

    /// `x != y` over two variables; bit `v` of a domain is set while the variable may take `v`.
    struct NotEqual;

    impl Propagator<[u16; 2]> for NotEqual {
        fn propagate(&mut self, domains: &mut [u16; 2]) -> Option<bool> {
            let mut changed = false;

            for (fixed, other) in [(0, 1), (1, 0)] {
                let value = domains[fixed];

                if value.is_power_of_two() && domains[other] & value != 0 {
                    domains[other] &= !value;
                    changed = true;
                }
            }

            if domains.contains(&0) {
                None
            } else {
                Some(changed)
            }
        }
    }

    /// Runs `propagator` until it changes nothing, the way a solver drives it.
    fn fixpoint<D, P: Propagator<D>>(propagator: &mut P, domains: &mut D) -> Option<u32> {
        let mut rounds = 0;
        while propagator.propagate(domains)? {
            rounds += 1;
        }

        Some(rounds)
    }

    /// A fixed variable removes its value from the other domain, then propagation settles.
    #[test]
    fn fixed_value_is_pruned_until_fixpoint() {
        let mut domains = [1 << 3, 1 << 3 | 1 << 5];
        assert_eq!(fixpoint(&mut NotEqual, &mut domains), Some(1));
        assert_eq!(domains, [1 << 3, 1 << 5]);
    }

    /// Two variables fixed to the same value empty a domain, which is a conflict.
    #[test]
    fn equal_fixed_values_conflict() {
        let mut domains = [1 << 2, 1 << 2];
        assert_eq!(fixpoint(&mut NotEqual, &mut domains), None);
    }
}
