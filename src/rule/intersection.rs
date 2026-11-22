//! [`GroupIntersection`]: reasoning across two overlapping all-different groups (Sudoku's
//! locked candidates).

use alloc::vec::Vec;

use super::subset::{collect_groups, is_permutation};
use crate::{Domain, Domains, Inconsistent, Rule, Var};

/// A value that group `A` must use, and can only take inside its overlap with group `B`, is
/// used by that overlap, so the rest of `B` cannot take it.
///
/// Sudoku players call this *locked candidates*: *pointing* (a block's candidates for a digit
/// lie in one row) and *claiming* or *box/line reduction* (a row's candidates lie in one
/// block). The default name is "group intersection"; [`GroupIntersection::with_name`] sets a
/// domain's own term. It applies to any pair of all-different
/// groups that share at least two variables, such as irregular regions or extra diagonals.
/// Like [`ValueSubset`](crate::ValueSubset), it only reasons from groups whose values must
/// all be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupIntersection {
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The all-different groups.
    groups: Vec<Vec<Var>>,
    /// The rule's [`Rule::name`].
    name: &'static str,
    /// The ordered pairs `(a, b)` of indices into [`GroupIntersection::groups`] whose groups share
    /// at least two variables.
    pairs: Vec<(usize, usize)>,
}

impl GroupIntersection {
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
            name: "group intersection",
            pairs,
        }
    }

    /// Names the rule in a domain's own terms, such as "pointing pair" for Sudoku.
    #[must_use]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
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

impl<D: Domain> Rule<D> for GroupIntersection {
    fn name(&self) -> &str {
        self.name
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

/// Unit tests for [`GroupIntersection`].
#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::GroupIntersection;
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

        GroupIntersection::new(0, [block, row])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[cells[4]], 0b0111);
        assert_eq!(domains[cells[5]], 0b0111);
        assert_eq!(domains[cells[0]], 0b1111);
    }

    /// Groups sharing a single variable form no pair: that case is a hidden single.
    #[test]
    fn single_overlap_is_ignored() {
        let rule = GroupIntersection::new(
            0,
            [
                vec![Var::new(0), Var::new(1)],
                vec![Var::new(1), Var::new(2)],
            ],
        );
        assert_eq!(rule.pairs, []);
        assert_eq!(Rule::<u16>::name(&rule), "group intersection");
    }
}
