//! [`AllDifferent`]: pairwise distinct values, with full Hall-set reasoning.

mod graph;

use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Propagator, Var};
pub use graph::MatchingScratch;

/// The variables take pairwise distinct values.
///
/// Propagation is complete (generalised arc consistency, Régin 1994): afterwards every value
/// left in a domain is part of some assignment of distinct values. This subsumes all Hall-set
/// reasoning: a fixed value leaves the other domains, `k` variables sharing `k` values keep
/// those values to themselves, and a value that only one variable can still take goes to it
/// when the values must all be used. It fails as soon as fewer values than variables remain.
///
/// Each run builds a matching between variables and values, in `O(n · e)` for `n` variables
/// and `e` variable–value pairs. Its buffers live in [`MatchingScratch`] and are reused.
///
/// # Examples
///
/// ```
/// use constraint_search::{AllDifferent, Domains, MatchingScratch, Propagator};
///
/// let mut domains = Domains::<u16>::new();
/// let x = domains.add(0b011);
/// let y = domains.add(0b011);
/// let z = domains.add(0b111);
///
/// // x and y use up the values 0 and 1, so z must be 2.
/// AllDifferent::new([x, y, z]).propagate(&mut domains, &mut MatchingScratch::default())?;
/// assert_eq!(domains.value(z), Some(2));
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllDifferent {
    /// The constrained variables. Never holds a variable twice.
    vars: Vec<Var>,
}

impl AllDifferent {
    /// All-different over `vars`.
    ///
    /// # Panics
    ///
    /// Panics if a variable appears twice.
    #[must_use]
    pub fn new(vars: impl IntoIterator<Item = Var>) -> Self {
        let vars: Vec<Var> = vars.into_iter().collect();

        crate::assert_distinct(&vars, "all-different lists a variable twice");

        Self { vars }
    }
}

impl<D: Domain> Propagator<D> for AllDifferent {
    type Scratch = MatchingScratch;

    fn scope(&self) -> &[Var] {
        &self.vars
    }

    fn propagate(
        &self,
        domains: &mut Domains<D>,
        graph: &mut MatchingScratch,
    ) -> Result<(), Inconsistent> {
        graph.build(domains, &self.vars);
        if !graph.match_all() {
            return Err(Inconsistent);
        }

        for &(position, value) in graph.unsupported() {
            domains.remove(self.vars[position], value)?;
        }

        Ok(())
    }
}

/// Tests [`AllDifferent`] against brute-force enumeration.
#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::{AllDifferent, MatchingScratch};
    use crate::testing::{Rng, check_against_brute_force};
    use crate::{Domains, Inconsistent, Propagator};

    /// Is every value of `assignment` different from the others?
    fn distinct(assignment: &[u32]) -> bool {
        assignment
            .iter()
            .enumerate()
            .all(|(i, a)| assignment[i + 1..].iter().all(|b| a != b))
    }

    /// On random small instances, propagation keeps exactly the values with a support.
    #[test]
    fn matches_brute_force() {
        let mut rng = Rng::new(7);
        for _ in 0..2000 {
            let vars = 1 + rng.below(5);
            check_against_brute_force(&mut rng, vars, 6, distinct, AllDifferent::new);
        }
    }

    /// A non-permutation keeps a value only one variable can take: it need not be used.
    #[test]
    fn unused_values_stay() {
        let mut domains = Domains::<u16>::new();
        let x = domains.add(0b111);
        let y = domains.add(0b011);

        AllDifferent::new([x, y])
            .propagate(&mut domains, &mut MatchingScratch::default())
            .unwrap();
        assert_eq!((domains[x], domains[y]), (0b111, 0b011));
    }

    /// Three variables over two values fail by pigeonhole before any is fixed.
    #[test]
    fn pigeonhole_fails() {
        let mut domains = Domains::<u16>::new();
        let vars: Vec<_> = (0..3).map(|_| domains.add(0b11)).collect();

        assert_eq!(
            AllDifferent::new(vars).propagate(&mut domains, &mut MatchingScratch::default()),
            Err(Inconsistent)
        );
    }

    /// Listing a variable twice is rejected at construction.
    #[test]
    #[should_panic(expected = "twice")]
    fn duplicate_var_panics() {
        let mut domains = Domains::<u16>::new();
        let x = domains.add(0b11);
        let _ = AllDifferent::new([x, x]);
    }
}
