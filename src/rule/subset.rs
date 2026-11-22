//! Subset rules over all-different groups: [`NakedSubset`] and [`HiddenSubset`].

use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Rule, Var};

/// `k` variables of a group whose domains together hold only `k` values use up those values,
/// so no other variable of the group can take them.
///
/// Size 1 is the *naked single* (a fixed variable's value leaves its group), size 2 the
/// *naked pair*, then *triple* and *quad*. Each group is an all-different scope. The rule
/// considers only variables with at most `size` values left, and for sizes above 1 only
/// unfixed ones, so each size reports its own technique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NakedSubset {
    /// The number of variables in the subset. At least 1.
    size: usize,
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The all-different groups the rule searches.
    groups: Vec<Vec<Var>>,
}

impl NakedSubset {
    /// The naked subset rule of `size` variables over `groups`, costing `cost`.
    ///
    /// # Panics
    ///
    /// Panics if `size` is 0.
    #[must_use]
    pub fn new<G: IntoIterator<Item = Var>>(
        size: usize,
        cost: u32,
        groups: impl IntoIterator<Item = G>,
    ) -> Self {
        assert!(size > 0, "a subset needs at least one variable");

        Self {
            size,
            cost,
            groups: collect_groups(groups),
        }
    }

    /// Applies the rule to one group.
    fn apply_to<D: Domain>(
        &self,
        group: &[Var],
        domains: &mut Domains<D>,
    ) -> Result<(), Inconsistent> {
        let lengths = if self.size == 1 { 1..=1 } else { 2..=self.size };
        let candidates: Vec<Var> = group
            .iter()
            .copied()
            .filter(|&var| lengths.contains(&(domains[var].len() as usize)))
            .collect();

        for_each_combination(candidates.len(), self.size, |chosen| {
            let subset: Vec<Var> = chosen.iter().map(|&i| candidates[i]).collect();
            let values = subset
                .iter()
                .fold(D::empty(), |union, &var| union.union(domains[var]));
            if values.len() as usize != self.size {
                return Ok(());
            }

            for &var in group.iter().filter(|var| !subset.contains(var)) {
                domains.remove_all(var, values)?;
            }

            Ok(())
        })
    }
}

impl<D: Domain> Rule<D> for NakedSubset {
    fn name(&self) -> &'static str {
        match self.size {
            1 => "naked single",
            2 => "naked pair",
            3 => "naked triple",
            4 => "naked quad",
            _ => "naked subset",
        }
    }

    fn cost(&self) -> u32 {
        self.cost
    }

    fn apply(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent> {
        self.groups
            .iter()
            .try_for_each(|group| self.apply_to(group, domains))
    }
}

/// `k` values that only `k` variables of a group can still take must go to those variables,
/// so those variables can take no other value.
///
/// Size 1 is the *hidden single*, then *hidden pair*, *triple* and *quad*.
///
/// The rule is sound only where every value must be used: groups whose variables together
/// can take exactly as many values as there are variables, as in a Sudoku row. Other groups
/// are skipped. (In `x ∈ {1, 2, 3}, y ∈ {1, 2}` only `x` can take 3, yet `x = 1, y = 2` is a
/// solution.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenSubset {
    /// The number of values in the subset. At least 1.
    size: usize,
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The all-different groups the rule searches.
    groups: Vec<Vec<Var>>,
}

impl HiddenSubset {
    /// The hidden subset rule of `size` values over `groups`, costing `cost`.
    ///
    /// # Panics
    ///
    /// Panics if `size` is 0.
    #[must_use]
    pub fn new<G: IntoIterator<Item = Var>>(
        size: usize,
        cost: u32,
        groups: impl IntoIterator<Item = G>,
    ) -> Self {
        assert!(size > 0, "a subset needs at least one value");

        Self {
            size,
            cost,
            groups: collect_groups(groups),
        }
    }

    /// Applies the rule to one group.
    fn apply_to<D: Domain>(
        &self,
        group: &[Var],
        domains: &mut Domains<D>,
    ) -> Result<(), Inconsistent> {
        if !is_permutation(group, domains) {
            return Ok(());
        }

        let candidates = self.candidate_values(group, domains);
        for_each_combination(candidates.len(), self.size, |chosen| {
            let mut values = D::empty();
            let mut holders: Vec<Var> = Vec::new();
            for (value, positions) in chosen.iter().map(|&i| &candidates[i]) {
                values.insert(*value);
                for var in positions {
                    if !holders.contains(var) {
                        holders.push(*var);
                    }
                }
            }

            if holders.len() != self.size {
                return Ok(());
            }

            for &var in &holders {
                domains.restrict(var, values)?;
            }

            Ok(())
        })
    }

    /// The values of `group` that `1..=size` unfixed variables can take (exactly one for
    /// size 1), with those variables.
    fn candidate_values<D: Domain>(
        &self,
        group: &[Var],
        domains: &Domains<D>,
    ) -> Vec<(u32, Vec<Var>)> {
        let counts = if self.size == 1 { 1..=1 } else { 2..=self.size };
        let union = group
            .iter()
            .fold(D::empty(), |union, &var| union.union(domains[var]));

        union
            .values()
            .map(|value| {
                let holders = group
                    .iter()
                    .copied()
                    .filter(|&var| domains[var].contains(value))
                    .collect::<Vec<_>>();
                (value, holders)
            })
            .filter(|(_, holders)| counts.contains(&holders.len()))
            .filter(|(_, holders)| holders.iter().any(|&var| domains[var].len() > 1))
            .collect()
    }
}

impl<D: Domain> Rule<D> for HiddenSubset {
    fn name(&self) -> &'static str {
        match self.size {
            1 => "hidden single",
            2 => "hidden pair",
            3 => "hidden triple",
            4 => "hidden quad",
            _ => "hidden subset",
        }
    }

    fn cost(&self) -> u32 {
        self.cost
    }

    fn apply(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent> {
        self.groups
            .iter()
            .try_for_each(|group| self.apply_to(group, domains))
    }
}

/// Returns `true` if the variables of `group` can together take exactly as many values as
/// there are variables, so that every value must be used.
pub(super) fn is_permutation<D: Domain>(group: &[Var], domains: &Domains<D>) -> bool {
    let union = group
        .iter()
        .fold(D::empty(), |union, &var| union.union(domains[var]));

    union.len() as usize == group.len()
}

/// Collects the groups of a rule constructor into owned vectors.
pub(super) fn collect_groups<G: IntoIterator<Item = Var>>(
    groups: impl IntoIterator<Item = G>,
) -> Vec<Vec<Var>> {
    groups
        .into_iter()
        .map(|group| group.into_iter().collect())
        .collect()
}

/// Calls `visit` with every `k`-element subset of `0..n` as ascending indices, stopping at
/// the first error.
fn for_each_combination(
    n: usize,
    k: usize,
    mut visit: impl FnMut(&[usize]) -> Result<(), Inconsistent>,
) -> Result<(), Inconsistent> {
    if k > n {
        return Ok(());
    }

    let mut chosen: Vec<usize> = (0..k).collect();
    loop {
        visit(&chosen)?;

        let Some(last) = (0..k).rev().find(|&i| chosen[i] < n - k + i) else {
            return Ok(());
        };

        chosen[last] += 1;
        for i in last + 1..k {
            chosen[i] = chosen[i - 1] + 1;
        }
    }
}

/// Unit tests for the subset rules.
#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{HiddenSubset, NakedSubset, for_each_combination};
    use crate::{Domains, Rule, Var};

    /// A group of variables with the given domains.
    fn group(domains_of: &[u16]) -> (Domains<u16>, Vec<Var>) {
        let mut domains = Domains::new();
        let vars = domains_of
            .iter()
            .map(|&domain| domains.add(domain))
            .collect();

        (domains, vars)
    }

    /// Combinations come in lexicographic order, and none exist for `k > n`.
    #[test]
    fn combinations() {
        let mut seen = vec![];
        for_each_combination(4, 2, |chosen| {
            seen.push((chosen[0], chosen[1]));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]);

        for_each_combination(1, 2, |_| panic!("no subset of size 2")).unwrap();
    }

    /// A naked pair removes its two values from the rest of the group.
    #[test]
    fn naked_pair() {
        let (mut domains, vars) = group(&[0b0011, 0b0011, 0b0111, 0b1111]);
        NakedSubset::new(2, 0, [vars.clone()])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[vars[2]], 0b0100);
        assert_eq!(domains[vars[3]], 0b1100);
    }

    /// A hidden pair strips the other values from its two variables.
    #[test]
    fn hidden_pair() {
        let (mut domains, vars) = group(&[0b1111, 0b1111, 0b1100, 0b1100]);
        HiddenSubset::new(2, 0, [vars.clone()])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[vars[0]], 0b0011);
        assert_eq!(domains[vars[1]], 0b0011);
    }

    /// Hidden reasoning leaves groups alone whose values need not all be used.
    #[test]
    fn hidden_single_skips_non_permutation() {
        let (mut domains, vars) = group(&[0b111, 0b011]);
        HiddenSubset::new(1, 0, [vars.clone()])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[vars[0]], 0b111);
    }

    /// Each size reports its own technique name.
    #[test]
    fn names() {
        let naked = [
            "naked single",
            "naked pair",
            "naked triple",
            "naked quad",
            "naked subset",
        ];
        let hidden = [
            "hidden single",
            "hidden pair",
            "hidden triple",
            "hidden quad",
            "hidden subset",
        ];
        for (size, (naked, hidden)) in (1..).zip(naked.into_iter().zip(hidden)) {
            let groups = [Vec::<Var>::new()];
            assert_eq!(
                Rule::<u16>::name(&NakedSubset::new(size, 0, groups.clone())),
                naked
            );
            assert_eq!(
                Rule::<u16>::name(&HiddenSubset::new(size, 0, groups)),
                hidden
            );
        }
    }
}
