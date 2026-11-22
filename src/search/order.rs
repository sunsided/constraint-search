//! Branching heuristics: [`VarOrder`] picks the variable, [`ValueOrder`] orders its values.

use alloc::vec::Vec;

use crate::propagator::Queue;
use crate::{Domain, Domains, Model, Var};

/// How search picks the variable to branch on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VarOrder {
    /// The unfixed variable with the fewest values left ("minimum remaining values",
    /// fail-first); ties go to the lowest index.
    #[default]
    Mrv,
    /// The unfixed variable with the lowest index.
    Input,
}

impl VarOrder {
    /// The variable to branch on, or `None` if every variable is fixed.
    pub(super) fn pick<D: Domain>(self, domains: &Domains<D>) -> Option<Var> {
        let mut unfixed = domains.vars().filter(|&var| domains[var].len() > 1);
        match self {
            Self::Mrv => unfixed.min_by_key(|&var| domains[var].len()),
            Self::Input => unfixed.next(),
        }
    }
}

/// How search orders the values of the branching variable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueOrder {
    /// Smallest value first.
    #[default]
    Ascending,
    /// Largest value first.
    Descending,
    /// The value that leaves the most values in all domains after propagation first
    /// ("least constraining value"). Values that propagation refutes are skipped. Costs one
    /// propagation per value, so it pays off when wrong choices are expensive.
    Lcv,
}

impl ValueOrder {
    /// The values of `var` to try, in order.
    pub(super) fn order<D: Domain>(
        self,
        var: Var,
        model: &Model<D>,
        domains: &mut Domains<D>,
        queue: &mut Queue,
    ) -> Vec<u32> {
        let mut values: Vec<u32> = domains[var].values().collect();
        match self {
            Self::Ascending => {}
            Self::Descending => values.reverse(),
            Self::Lcv => values = least_constraining(var, values, model, domains, queue),
        }

        values
    }
}

/// `values` of `var` that survive propagation, the one leaving the most values first.
fn least_constraining<D: Domain>(
    var: Var,
    values: Vec<u32>,
    model: &Model<D>,
    domains: &mut Domains<D>,
    queue: &mut Queue,
) -> Vec<u32> {
    let mut scored: Vec<(u64, u32)> = Vec::with_capacity(values.len());
    for value in values {
        domains.push_level();
        let survives = domains
            .assign(var, value)
            .and_then(|_| queue.propagate_changes(model, domains));
        if survives.is_ok() {
            let left = domains
                .vars()
                .map(|other| u64::from(domains[other].len()))
                .sum();
            scored.push((left, value));
        }

        domains.pop_level();
    }

    scored.sort_by_key(|&(left, _)| core::cmp::Reverse(left));
    scored.into_iter().map(|(_, value)| value).collect()
}
