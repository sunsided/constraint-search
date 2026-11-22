//! Questions about problems with many solutions: which values are possible or certain
//! ([`Solver::possible_values`], [`Solver::backbone`]), how likely each value is
//! ([`Solver::marginals`]), and a random solution ([`Solver::sample`]).
//!
//! Propagation and every rule stay sound when a problem has many solutions; only rules with
//! [`Rule::assumes_unique`](crate::Rule::assumes_unique) must stay off.

use alloc::vec;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use search_core::Budget;

use super::{Outcome, Report, Solver, Stats, Status, ValueOrder};
use crate::{Domain, Domains, Var};

/// How often each value occurs across the solutions a [`Solver::marginals`] call enumerated,
/// optionally weighted.
///
/// With every solution enumerated ([`Marginals::is_exact`]), [`Marginals::probability`] is the
/// exact probability under a uniform (or the given) weighting of the solutions; otherwise it
/// estimates from the solutions seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Marginals {
    /// Values per variable in [`Marginals::weights`]: one more than the largest value seen.
    stride: usize,
    /// `weights[var * stride + value]`: the summed weight of the solutions with that value.
    weights: Vec<f64>,
    /// The summed weight of all solutions seen.
    total: f64,
    /// The number of solutions seen.
    solutions: u64,
    /// `true` if the search space was exhausted, so every solution was seen.
    exact: bool,
}

impl Marginals {
    /// The weighted share of solutions in which `var` takes `value`, between 0 and 1; 0 when
    /// no solution was seen.
    #[must_use]
    pub fn probability(&self, var: Var, value: u32) -> f64 {
        let value = value as usize;
        if self.total <= 0.0 || value >= self.stride {
            return 0.0;
        }

        self.weights[var.index() * self.stride + value] / self.total
    }

    /// The number of solutions enumerated.
    #[must_use]
    pub fn solutions(&self) -> u64 {
        self.solutions
    }

    /// The summed weight of the solutions enumerated.
    #[must_use]
    pub fn total_weight(&self) -> f64 {
        self.total
    }

    /// Returns `true` if every solution was enumerated, so the probabilities are exact; `false`
    /// if more solutions exist than the limit allowed, or the budget ran out.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.exact
    }
}

impl<D: Domain> Solver<'_, D> {
    /// Sets the seed of [`ValueOrder::Random`]. The same seed on the same problem gives the
    /// same order.
    pub fn set_seed(&mut self, seed: u64) {
        self.ordering.seed(seed);
    }

    /// Draws a random solution: up to `restarts` searches in random value order, each under
    /// its own copy of `budget`, until one finds a solution.
    ///
    /// Restarting with fresh random orders keeps a search from getting stuck in one hard
    /// region, and spreads the solutions found. They are not drawn exactly uniformly: a
    /// solution that many value choices lead to comes up more often. Successive calls continue
    /// the random sequence; [`Solver::set_seed`] makes them reproducible. A sample is a
    /// determinization of hidden information, e.g. a deal of the unseen cards for Perfect
    /// Information Monte Carlo in game-tree search.
    ///
    /// Returns [`Outcome::Unsatisfiable`] as soon as one search exhausts its space without a
    /// solution, and [`Outcome::Exhausted`] if every restart ran out of budget. With zero
    /// restarts nothing is searched, and the outcome is [`Outcome::Exhausted`]: no attempt had
    /// any budget. Like [`Solver::solve`], this abandons a search in progress. A hint set with
    /// [`Solver::set_hint`] is ignored here and stays set afterwards.
    pub fn sample<B: Budget<Stats> + Clone>(
        &mut self,
        restarts: u32,
        budget: B,
    ) -> Report<Outcome> {
        self.abandon();
        let order = core::mem::replace(&mut self.value_order, ValueOrder::Random);
        // A hint would put its solution first on every restart and undo the shuffle.
        let hint = core::mem::take(&mut self.hint);
        let mut stats = Stats::default();
        let mut outcome = Outcome::Exhausted;

        // One budget per restart: clones for all but the last, which takes the original.
        let budgets = core::iter::repeat_n(budget, usize::try_from(restarts).unwrap_or(usize::MAX));
        for budget in budgets {
            let report = self.solve(budget);
            stats = stats.plus(report.stats);
            outcome = report.outcome;
            if outcome != Outcome::Exhausted {
                break;
            }
        }

        self.value_order = order;
        self.hint = hint;
        Report { outcome, stats }
    }

    /// The values each variable takes in at least one solution of the current domains, or
    /// `None` if `budget` stopped the search first. Without solutions, every domain is empty.
    ///
    /// A value outside its variable's result is impossible: no solution uses it. This is exact,
    /// unlike propagation, which may keep values that no solution uses. Each solution found
    /// supports all its values at once, so only values not yet seen in a solution cost a
    /// search of their own. The budget sees the counters of all those searches together.
    ///
    /// Like [`Solver::solve`], this abandons a search in progress and answers for the domains
    /// that search began from.
    pub fn possible_values(&mut self, budget: impl Budget<Stats>) -> Report<Option<Vec<D>>> {
        // Abandon first, so the candidates and the searches see the same domains.
        self.abandon();
        let mut budget = budget;
        let mut stats = Stats::default();
        let mut possible = vec![D::empty(); self.domains.len()];
        let candidates: Vec<D> = self.domains.vars().map(|var| self.domains[var]).collect();

        let first = self.witness(None, &mut budget, &mut stats);
        let outcome = match first {
            Witness::Found(values) => {
                support(&mut possible, &values);
                self.test_candidates(&candidates, &mut possible, &mut budget, &mut stats)
                    .then_some(possible)
            }
            Witness::None => Some(possible),
            Witness::Exhausted => None,
        };

        Report { outcome, stats }
    }

    /// The value of each variable that is the same in every solution, `None` for the others:
    /// the certain facts of an undetermined problem, such as the cards whose location is known
    /// or the Minesweeper cells that are safe or mines for sure. The outer `None` means that
    /// `budget` stopped the search first. Without solutions, no variable has a value.
    ///
    /// For belief tracking, record each observation as it arrives (restrict or fix variables
    /// through [`Solver::domains_mut`]) and ask again: the solutions left are the belief
    /// state, and the backbone is what is known for certain.
    pub fn backbone(&mut self, budget: impl Budget<Stats>) -> Report<Option<Vec<Option<u32>>>> {
        let report = self.possible_values(budget);
        let outcome = report
            .outcome
            .map(|possible| possible.iter().map(Domain::value).collect());

        Report {
            outcome,
            stats: report.stats,
        }
    }

    /// How often each value occurs among the solutions, enumerating up to `limit` of them.
    ///
    /// [`Marginals::is_exact`] is `true` exactly when no solution beyond those counted exists;
    /// telling that apart costs finding one solution past the limit. Like [`Solver::solve`],
    /// this abandons a search in progress. A limit of 0 counts nothing and is never exact.
    ///
    /// For example the probability that a hidden card is in West's hand, or that a Minesweeper
    /// cell is a mine. Choosing the probe whose outcome splits the solutions best (the safest
    /// click, a Mastermind guess) is then an expectimax step over these counts.
    pub fn marginals(&mut self, limit: u64, budget: impl Budget<Stats>) -> Report<Marginals> {
        self.weighted_marginals(limit, budget, |_| 1.0)
    }

    /// Like [`Solver::marginals`], with each solution weighted by `weight`, for non-uniform
    /// priors such as the hands that fit a Bridge bidding sequence best. Weights should be
    /// non-negative.
    pub fn weighted_marginals(
        &mut self,
        limit: u64,
        budget: impl Budget<Stats>,
        mut weight: impl FnMut(&Domains<D>) -> f64,
    ) -> Report<Marginals> {
        // Abandon first, so the value range covers every domain the search will see.
        self.abandon();
        let stride = self.value_stride();
        let mut marginals = Marginals {
            stride,
            weights: vec![0.0; self.domains.len() * stride],
            total: 0.0,
            solutions: 0,
            exact: false,
        };

        let report = if limit == 0 {
            Report {
                outcome: Status::Stopped,
                stats: Stats::default(),
            }
        } else {
            self.for_each_solution(budget, |domains| {
                // A solution past the limit only proves the enumeration incomplete.
                if marginals.solutions == limit {
                    return ControlFlow::Break(());
                }

                add_solution(&mut marginals, domains, weight(domains));
                ControlFlow::Continue(())
            })
        };

        marginals.exact = report.outcome == Status::Complete;
        Report {
            outcome: marginals,
            stats: report.stats,
        }
    }

    /// One more than the largest value in any current domain.
    fn value_stride(&self) -> usize {
        self.domains
            .vars()
            .filter_map(|var| {
                let domain = self.domains[var];
                domain.values().last()
            })
            .max()
            .map_or(0, |largest| largest as usize + 1)
    }

    /// Tests every candidate value not yet supported, adding the support of each solution it
    /// finds; returns `false` if the budget stopped it.
    fn test_candidates(
        &mut self,
        candidates: &[D],
        possible: &mut [D],
        budget: &mut impl Budget<Stats>,
        stats: &mut Stats,
    ) -> bool {
        for (index, &domain) in candidates.iter().enumerate() {
            let var = Var::new(u32::try_from(index).expect("fewer than u32::MAX variables"));
            for value in domain.values() {
                if possible[index].contains(value) {
                    continue;
                }

                match self.witness(Some((var, value)), budget, stats) {
                    Witness::Found(values) => support(possible, &values),
                    Witness::None => {}
                    Witness::Exhausted => return false,
                }
            }
        }

        true
    }

    /// Searches for one solution, with `fixed` assigned first if given.
    fn witness(
        &mut self,
        fixed: Option<(Var, u32)>,
        budget: &mut impl Budget<Stats>,
        stats: &mut Stats,
    ) -> Witness {
        self.abandon();
        self.domains.push_level();

        let mut found = None;
        let base = *stats;
        let report = if fixed.is_none_or(|(var, value)| self.domains.assign(var, value).is_ok()) {
            self.for_each_solution(
                |current: &Stats| budget.exhausted(&base.plus(*current)),
                |domains| {
                    found = domains.solution();
                    ControlFlow::Break(())
                },
            )
        } else {
            Report {
                outcome: Status::Complete,
                stats: Stats::default(),
            }
        };

        self.domains.pop_level();
        *stats = base.plus(report.stats);
        match (found, report.outcome) {
            (Some(values), _) => Witness::Found(values),
            (None, Status::Exhausted) => Witness::Exhausted,
            (None, _) => Witness::None,
        }
    }
}

/// The result of looking for one solution.
enum Witness {
    /// A solution: the value of every variable.
    Found(Vec<u32>),
    /// The search space holds no solution.
    None,
    /// The budget ran out first.
    Exhausted,
}

/// Marks every value of `solution` as possible.
fn support<D: Domain>(possible: &mut [D], solution: &[u32]) {
    for (domain, &value) in possible.iter_mut().zip(solution) {
        domain.insert(value);
    }
}

/// Adds the solution in `domains`, with weight `weight`, to `marginals`.
fn add_solution<D: Domain>(marginals: &mut Marginals, domains: &Domains<D>, weight: f64) {
    for var in domains.vars() {
        let value = domains.value(var).expect("a solution fixes every variable") as usize;
        marginals.weights[var.index() * marginals.stride + value] += weight;
    }

    marginals.total += weight;
    marginals.solutions += 1;
}
