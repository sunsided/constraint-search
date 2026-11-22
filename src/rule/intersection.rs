//! [`Intersection`]: locked candidates between two overlapping all-different groups.

use alloc::vec::Vec;

use super::subset::{collect_groups, is_permutation};
use crate::{Domain, Domains, Inconsistent, Rule, Var};

/// A value that group `A` must use, and can only take inside its overlap with group `B`, is
/// used by that overlap, so the rest of `B` cannot take it.
///
/// In Sudoku this is *pointing* (a block's candidates for a digit lie in one row) and
/// *claiming* (a row's candidates lie in one block). It applies to any pair of all-different
/// groups that share at least two variables, such as irregular regions or extra diagonals.
/// Like [`HiddenSubset`](crate::HiddenSubset), it only reasons from groups whose values must
/// all be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intersection {
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The all-different groups.
    groups: Vec<Vec<Var>>,
    /// The ordered pairs `(a, b)` of indices into [`Intersection::groups`] whose groups share
    /// at least two variables.
    pairs: Vec<(usize, usize)>,
}

impl Intersection {
    /// The intersection rule over `groups`, costing `cost`.
    #[must_use]
    pub fn new<G: IntoIterator<Item = Var>>(
        cost: u32,
        groups: impl IntoIterator<Item = G>,
    ) -> Self {
        let groups = collect_groups(groups);
        let mut pairs = Vec::new();
        for (a, first) in groups.iter().enumerate() {
            for (b, second) in groups.iter().enumerate() {
                let shared = first.iter().filter(|var| second.contains(var)).count();
                if a != b && shared >= 2 {
                    pairs.push((a, b));
                }
            }
        }

        Self {
            cost,
            groups,
            pairs,
        }
    }

    /// Removes from `outer \ inner` every value that `inner` must use but can only take in
    /// the variables it shares with `outer`.
    fn lock<D: Domain>(
        inner: &[Var],
        outer: &[Var],
        domains: &mut Domains<D>,
    ) -> Result<(), Inconsistent> {
        if !is_permutation(inner, domains) {
            return Ok(());
        }

        let union = inner
            .iter()
            .fold(D::empty(), |union, &var| union.union(domains[var]));
        for value in union.values() {
            let confined = inner
                .iter()
                .filter(|&&var| domains[var].contains(value))
                .all(|var| outer.contains(var));
            if !confined {
                continue;
            }

            for &var in outer.iter().filter(|var| !inner.contains(var)) {
                domains.remove(var, value)?;
            }
        }

        Ok(())
    }
}

impl<D: Domain> Rule<D> for Intersection {
    fn name(&self) -> &'static str {
        "intersection"
    }

    fn cost(&self) -> u32 {
        self.cost
    }

    fn apply(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent> {
        self.pairs
            .iter()
            .try_for_each(|&(a, b)| Self::lock(&self.groups[a], &self.groups[b], domains))
    }
}

/// Unit tests for [`Intersection`].
#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::Intersection;
    use crate::{Domains, Rule, Var};

    /// A box of four cells whose value 3 sits only in its top row removes 3 from the rest of
    /// that row.
    #[test]
    fn pointing() {
        let mut domains = Domains::<u16>::new();
        let cells: Vec<Var> = [0b1111, 0b1111, 0b0111, 0b0111, 0b1111, 0b1111]
            .into_iter()
            .map(|domain| domains.add(domain))
            .collect();
        let block = vec![cells[0], cells[1], cells[2], cells[3]];
        let row = vec![cells[0], cells[1], cells[4], cells[5]];

        Intersection::new(0, [block, row])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[cells[4]], 0b0111);
        assert_eq!(domains[cells[5]], 0b0111);
        assert_eq!(domains[cells[0]], 0b1111);
    }

    /// Groups sharing a single variable form no pair: that case is a hidden single.
    #[test]
    fn single_overlap_is_ignored() {
        let rule = Intersection::new(
            0,
            [
                vec![Var::new(0), Var::new(1)],
                vec![Var::new(1), Var::new(2)],
            ],
        );
        assert_eq!(rule.pairs, []);
    }
}
