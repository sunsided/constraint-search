//! Branching heuristics: [`VarOrder`] picks the variable, [`ValueOrder`] orders its values.

use alloc::vec::Vec;

use crate::propagator::Engine;
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
    pub(crate) fn pick<D: Domain>(self, domains: &Domains<D>) -> Option<Var> {
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
    /// ("least constraining value"). Values that propagation refutes are skipped and count as
    /// [`Stats::failures`](super::Stats::failures). Costs one propagation per value, so it pays
    /// off when wrong choices are expensive.
    Lcv,
    /// A random order, from the solver's seed ([`Solver::set_seed`](super::Solver::set_seed)).
    /// Repeated searches with different seeds sample different solutions; see
    /// [`Solver::sample`](super::Solver::sample).
    Random,
}

/// Scratch and state of the value orderings, owned by the solver and reused.
#[derive(Debug, Clone)]
pub(super) struct OrderState {
    /// For [`ValueOrder::Lcv`]: how many values each candidate leaves, and the value.
    pub(super) scores: Vec<(u64, u32)>,
    /// For [`ValueOrder::Random`]: the xorshift64 state. Never zero.
    pub(super) rng: u64,
}

impl Default for OrderState {
    fn default() -> Self {
        Self {
            scores: Vec::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
        }
    }
}

impl OrderState {
    /// Seeds the random ordering; a zero seed is replaced, since xorshift sticks at zero.
    pub(super) fn seed(&mut self, seed: u64) {
        self.rng = seed.max(1);
    }

    /// The next 64 random bits.
    fn next(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Shuffles `values` uniformly (Fisher–Yates).
    fn shuffle(&mut self, values: &mut [u32]) {
        for i in (1..values.len()).rev() {
            let bound = u64::try_from(i + 1).expect("a domain has fewer than 2^64 values");
            let j = usize::try_from(self.next() % bound).expect("below a usize bound");
            values.swap(i, j);
        }
    }
}

impl ValueOrder {
    /// Pushes the values of `var` to try onto `values`, in order, and returns how many values
    /// the ordering itself refuted.
    pub(super) fn order<D: Domain>(
        self,
        var: Var,
        model: &Model<D>,
        domains: &mut Domains<D>,
        engine: &mut Engine,
        values: &mut Vec<u32>,
        state: &mut OrderState,
    ) -> u64 {
        let start = values.len();
        match self {
            Self::Ascending => values.extend(domains[var].values()),
            Self::Descending => {
                values.extend(domains[var].values());
                values[start..].reverse();
            }
            Self::Lcv => {
                let refuted = score(var, model, domains, engine, &mut state.scores);
                values.extend(state.scores.iter().map(|&(_, value)| value));
                return refuted;
            }
            Self::Random => {
                values.extend(domains[var].values());
                state.shuffle(&mut values[start..]);
            }
        }

        0
    }
}

/// Scores each value of `var` that survives propagation by the values it leaves, most first
/// (ties by ascending value), and returns how many values propagation refuted.
fn score<D: Domain>(
    var: Var,
    model: &Model<D>,
    domains: &mut Domains<D>,
    engine: &mut Engine,
    scores: &mut Vec<(u64, u32)>,
) -> u64 {
    let candidates = domains[var];
    scores.clear();
    for value in candidates.values() {
        domains.push_level();
        let survives = domains
            .assign(var, value)
            .and_then(|_| engine.propagate_changes(model, domains));
        if survives.is_ok() {
            let left = domains
                .vars()
                .map(|other| u64::from(domains[other].len()))
                .sum();
            scores.push((left, value));
        }

        domains.pop_level();
    }

    // Unstable sort: it never allocates, and the value breaks ties deterministically.
    scores.sort_unstable_by_key(|&(left, value)| (core::cmp::Reverse(left), value));
    u64::from(candidates.len()) - u64::try_from(scores.len()).expect("fewer than 2^64 values")
}
