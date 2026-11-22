//! Subset rules over all-different groups: [`VariableSubset`] and [`ValueSubset`], the
//! Hall-set reasoning a person does by hand (Sudoku calls them naked and hidden subsets).

use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Rule, Var};

/// `k` variables of an all-different group whose domains together hold only `k` values use
/// up those values, so no other variable of the group can take them.
///
/// Size 1 is a fixed variable whose value leaves the rest of its group. The rule considers
/// only variables with at most `size` values left, and for sizes above 1 only unfixed ones,
/// so each size reports its own technique. It is sound for every all-different group,
/// whether or not the group must use all its values.
///
/// Default names are "variable single", "variable pair", "variable triple", "variable quad"
/// and "variable subset"; [`VariableSubset::with_name`] sets a domain's own term. Sudoku
/// players call these *naked singles, pairs, triples and quads*; in a logic-grid puzzle it is
/// "these two people own the only two remaining pets".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSubset {
    /// The number of variables in the subset. At least 1.
    size: usize,
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The all-different groups the rule searches.
    groups: Vec<Vec<Var>>,
    /// The rule's [`Rule::name`].
    name: &'static str,
}

impl VariableSubset {
    /// The subset rule of `size` variables over `groups`, costing `cost`.
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

        let name = [
            "variable single",
            "variable pair",
            "variable triple",
            "variable quad",
        ]
        .get(size - 1)
        .copied()
        .unwrap_or("variable subset");
        Self {
            size,
            cost,
            groups: collect_groups(groups),
            name,
        }
    }

    /// Names the rule in a domain's own terms, such as "naked pair" for Sudoku.
    #[must_use]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
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
            let in_subset = |var: &Var| chosen.iter().any(|&i| candidates[i] == *var);
            let values = chosen
                .iter()
                .fold(D::empty(), |union, &i| union.union(domains[candidates[i]]));
            if values.len() as usize != self.size {
                return Ok(());
            }

            for &var in group.iter().filter(|var| !in_subset(var)) {
                domains.remove_all(var, values)?;
            }

            Ok(())
        })
    }
}

impl<D: Domain> Rule<D> for VariableSubset {
    fn name(&self) -> &str {
        self.name
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

/// `k` values that only `k` variables of an all-different group can still take must go to
/// those variables, so those variables can take no other value.
///
/// **Only sound where every value must be used:** groups whose variables together can take
/// exactly as many values as there are variables (a permutation), as in a Sudoku row or a
/// logic-grid attribute. Other groups are skipped. In `x ∈ {1, 2, 3}, y ∈ {1, 2}` only `x`
/// can take 3, yet `x = 1, y = 2` is a solution. Card-location and hidden-identity problems
/// are often not permutations; model them so that they are, or rely on [`VariableSubset`].
///
/// Default names are "value single", "value pair", "value triple", "value quad" and "value
/// subset"; [`ValueSubset::with_name`] sets a domain's own term. Sudoku players call these
/// *hidden singles, pairs, triples and quads*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueSubset {
    /// The number of values in the subset. At least 1.
    size: usize,
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The all-different groups the rule searches.
    groups: Vec<Vec<Var>>,
    /// The rule's [`Rule::name`].
    name: &'static str,
}

impl ValueSubset {
    /// The subset rule of `size` values over `groups`, costing `cost`.
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

        let name = ["value single", "value pair", "value triple", "value quad"]
            .get(size - 1)
            .copied()
            .unwrap_or("value subset");
        Self {
            size,
            cost,
            groups: collect_groups(groups),
            name,
        }
    }

    /// Names the rule in a domain's own terms, such as "hidden pair" for Sudoku.
    #[must_use]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
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
        let mut holders: Vec<Var> = Vec::with_capacity(self.size * self.size);
        for_each_combination(candidates.len(), self.size, |chosen| {
            let mut values = D::empty();
            holders.clear();
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

    /// The values of `group` that `1..=size` variables can take, at least one of them unfixed,
    /// with those variables.
    fn candidate_values<D: Domain>(
        &self,
        group: &[Var],
        domains: &Domains<D>,
    ) -> Vec<(u32, Vec<Var>)> {
        let counts = 1..=self.size;
        let union = domains.union_of(group);

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

impl<D: Domain> Rule<D> for ValueSubset {
    fn name(&self) -> &str {
        self.name
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
    domains.union_of(group).len() as usize == group.len()
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

    use super::{ValueSubset, VariableSubset, for_each_combination};
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

    /// A variable pair (a naked pair) removes its two values from the rest of the group.
    #[test]
    fn naked_pair() {
        let (mut domains, vars) = group(&[0b0011, 0b0011, 0b0111, 0b1111]);
        VariableSubset::new(2, 0, [vars.clone()])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[vars[2]], 0b0100);
        assert_eq!(domains[vars[3]], 0b1100);
    }

    /// A value pair (a hidden pair) strips the other values from its two variables.
    #[test]
    fn hidden_pair() {
        let (mut domains, vars) = group(&[0b1111, 0b1111, 0b1100, 0b1100]);
        ValueSubset::new(2, 0, [vars.clone()])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[vars[0]], 0b0011);
        assert_eq!(domains[vars[1]], 0b0011);
    }

    /// A value pair may include a value only one variable can take.
    #[test]
    fn hidden_pair_with_a_single() {
        let (mut domains, vars) = group(&[0b1111, 0b1110, 0b1100, 0b1100]);
        ValueSubset::new(2, 0, [vars.clone()])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[vars[0]], 0b0011);
        assert_eq!(domains[vars[1]], 0b0010);
    }

    /// Hidden reasoning leaves groups alone whose values need not all be used.
    #[test]
    fn hidden_single_skips_non_permutation() {
        let (mut domains, vars) = group(&[0b111, 0b011]);
        ValueSubset::new(1, 0, [vars.clone()])
            .apply(&mut domains)
            .unwrap();

        assert_eq!(domains[vars[0]], 0b111);
    }

    /// Each size reports its own technique name.
    #[test]
    fn names() {
        let variable = [
            "variable single",
            "variable pair",
            "variable triple",
            "variable quad",
        ];
        let value = ["value single", "value pair", "value triple", "value quad"];
        for (size, (variable, value)) in (1..).zip(variable.into_iter().zip(value)) {
            let groups = [Vec::<Var>::new()];
            assert_eq!(
                Rule::<u16>::name(&VariableSubset::new(size, 0, groups.clone())),
                variable
            );
            assert_eq!(Rule::<u16>::name(&ValueSubset::new(size, 0, groups)), value);
        }

        let groups = [Vec::<Var>::new()];
        assert_eq!(
            Rule::<u16>::name(&ValueSubset::new(5, 0, groups.clone())),
            "value subset"
        );
        let renamed = VariableSubset::new(2, 0, groups).with_name("naked pair");
        assert_eq!(Rule::<u16>::name(&renamed), "naked pair");
    }
}
