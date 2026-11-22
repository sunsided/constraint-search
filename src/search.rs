//! Backtracking search: the [`Solver`] that finds, counts and enumerates solutions of a
//! [`Model`].
//!
//! Each node picks an unfixed variable, tries its values in turn and propagates after each
//! choice; a conflict undoes the choice through the trail.

mod order;

use alloc::vec::Vec;
use core::ops::ControlFlow;

use search_core::Budget;

use crate::propagator::Queue;
use crate::{Domain, Domains, Inconsistent, Model};
pub use order::{ValueOrder, VarOrder};

/// Counters collected during one search.
///
/// Non-exhaustive: later versions may add counters. Start from [`Stats::default`] where a
/// value is needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    /// Search nodes entered, the root included.
    pub nodes: u64,
    /// Value choices that propagation refuted.
    pub failures: u64,
    /// Full assignments found.
    pub solutions: u64,
}

/// How a search ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A solution: the value of every variable, in index order.
    Solved(Vec<u32>),
    /// The search space is exhausted: no solution exists.
    Unsatisfiable,
    /// The budget stopped the search first.
    Exhausted,
}

/// How many solutions a count found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    /// The search space was exhausted: there are exactly this many solutions.
    Exact(u64),
    /// The count stopped at the limit or at the budget: there are at least this many.
    AtLeast(u64),
}

/// Why [`Solver::for_each_solution`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The search space is exhausted: every solution was reported.
    Complete,
    /// The callback asked to stop.
    Stopped,
    /// The budget stopped the search first.
    Exhausted,
}

/// What a search returns: its result and the counters that led to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report<T> {
    /// The result.
    pub outcome: T,
    /// The work the search did.
    pub stats: Stats,
}

/// Backtracking search over a shared [`Model`], with reusable working state.
///
/// The solver owns the current domains, their trail and the propagation queue, and keeps
/// their allocations from one solve to the next: [`Solver::reset`] returns to the model's
/// initial domains without reallocating. Search leaves the domains as it found them, so a
/// caller can fix some variables ([`Solver::domains_mut`]) and search many times from there.
///
/// The model is only borrowed and is `Sync`, so threads can each run a solver over one model.
/// Fixing different variables per solver splits the search tree into independent subtrees,
/// and differing [`VarOrder`]s and [`ValueOrder`]s make a portfolio. A shared stop flag goes
/// into the budget, which every node consults.
///
/// Each search node picks an unfixed variable, tries its values in turn and propagates after
/// each choice; a conflict undoes the choice through the trail.
///
/// ```mermaid
/// flowchart LR
///     node([node]) --> pick{unfixed variable?}
///     pick -->|none| solution([solution])
///     pick -->|"pick by VarOrder"| value[next value by ValueOrder]
///     value --> propagate[assign and propagate]
///     propagate -->|ok| node
///     propagate -->|conflict| undo[undo] --> value
/// ```
///
/// # Examples
///
/// The 3×3 Latin squares: each row and column holds 0, 1 and 2 once.
///
/// ```
/// use constraint_search::{AllDifferent, Count, Domain, Model, Solver};
///
/// let mut model = Model::<u16>::new();
/// let cells = model.add_vars(9, u16::range(3));
/// for i in 0..3 {
///     model.add(AllDifferent::new((0..3).map(|j| cells[3 * i + j])));
///     model.add(AllDifferent::new((0..3).map(|j| cells[3 * j + i])));
/// }
///
/// let mut solver = Solver::new(&model);
/// let report = solver.count(u64::MAX, |_: &_| false);
/// assert_eq!(report.outcome, Count::Exact(12));
///
/// // Fix the first row and the square is unique up to the remaining rows.
/// for (j, value) in [0, 1, 2].into_iter().enumerate() {
///     solver.domains_mut().assign(cells[j], value)?;
/// }
/// assert_eq!(solver.count(u64::MAX, |_: &_| false).outcome, Count::Exact(2));
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
pub struct Solver<'m, D> {
    /// The problem being solved.
    model: &'m Model<D>,
    /// The current domains and their trail.
    domains: Domains<D>,
    /// The propagation queue.
    queue: Queue,
    /// How to pick the variable to branch on.
    var_order: VarOrder,
    /// How to order the values of that variable.
    value_order: ValueOrder,
}

impl<'m, D: Domain> Solver<'m, D> {
    /// A solver over `model`, starting from its initial domains, branching with
    /// [`VarOrder::Mrv`] and [`ValueOrder::Ascending`].
    #[must_use]
    pub fn new(model: &'m Model<D>) -> Self {
        let mut solver = Self {
            model,
            domains: Domains::new(),
            queue: Queue::default(),
            var_order: VarOrder::default(),
            value_order: ValueOrder::default(),
        };
        solver.reset();

        solver
    }

    /// Sets how to pick the variable to branch on.
    #[must_use]
    pub fn with_var_order(mut self, order: VarOrder) -> Self {
        self.var_order = order;
        self
    }

    /// Sets how to order the values of the branching variable.
    #[must_use]
    pub fn with_value_order(mut self, order: ValueOrder) -> Self {
        self.value_order = order;
        self
    }

    /// Returns to the model's initial domains, keeping every allocation.
    pub fn reset(&mut self) {
        self.domains.reset_to(self.model.initial_domains());
    }

    /// The current domains.
    #[must_use]
    pub fn domains(&self) -> &Domains<D> {
        &self.domains
    }

    /// The current domains, to fix or restrict variables before a search.
    pub fn domains_mut(&mut self) -> &mut Domains<D> {
        &mut self.domains
    }

    /// Runs every propagator to a fixpoint on the current domains, keeping the result.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if the constraints cannot be satisfied. The domains are then
    /// partly propagated; [`Solver::reset`] starts over.
    pub fn propagate(&mut self) -> Result<(), Inconsistent> {
        self.queue.fixpoint(self.model, &mut self.domains)
    }

    /// Finds one solution.
    pub fn solve(&mut self, budget: impl Budget<Stats>) -> Report<Outcome> {
        let mut found = None;
        let report = self.for_each_solution(budget, |domains| {
            found = domains.solution();
            ControlFlow::Break(())
        });

        let outcome = match (report.outcome, found) {
            (_, Some(values)) => Outcome::Solved(values),
            (Status::Exhausted, None) => Outcome::Exhausted,
            (_, None) => Outcome::Unsatisfiable,
        };

        Report {
            outcome,
            stats: report.stats,
        }
    }

    /// Counts solutions, stopping at `limit`.
    ///
    /// A limit of 2 decides uniqueness: `Count::Exact(1)` means exactly one solution, and
    /// `Count::AtLeast(2)` more than one.
    pub fn count(&mut self, limit: u64, budget: impl Budget<Stats>) -> Report<Count> {
        let mut solutions = 0;
        let report = self.for_each_solution(budget, |_| {
            solutions += 1;
            if solutions >= limit {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });

        let outcome = match report.outcome {
            Status::Complete => Count::Exact(solutions),
            Status::Stopped | Status::Exhausted => Count::AtLeast(solutions),
        };

        Report {
            outcome,
            stats: report.stats,
        }
    }

    /// Calls `on_solution` with the domains of every solution, each holding one value per
    /// variable, until it returns [`ControlFlow::Break`] or the search ends.
    ///
    /// The domains are as they were before the call when it returns.
    pub fn for_each_solution(
        &mut self,
        budget: impl Budget<Stats>,
        on_solution: impl FnMut(&Domains<D>) -> ControlFlow<()>,
    ) -> Report<Status> {
        let mut search = Search {
            solver: self,
            budget,
            on_solution,
            stats: Stats::default(),
        };

        let outcome = search.root();
        Report {
            outcome,
            stats: search.stats,
        }
    }
}

/// Why a search stopped before exhausting its space.
enum Stop {
    /// The solution callback asked to stop.
    Caller,
    /// The budget ran out.
    Budget,
}

/// One running search: the solver, its budget and callback, and the counters so far.
struct Search<'s, 'm, D, B, F> {
    /// The solver whose domains are searched.
    solver: &'s mut Solver<'m, D>,
    /// Decides when to stop early.
    budget: B,
    /// Receives each solution.
    on_solution: F,
    /// The counters so far.
    stats: Stats,
}

impl<D, B, F> Search<'_, '_, D, B, F>
where
    D: Domain,
    B: Budget<Stats>,
    F: FnMut(&Domains<D>) -> ControlFlow<()>,
{
    /// Propagates the starting domains, searches below them and restores them.
    fn root(&mut self) -> Status {
        let solver = &mut *self.solver;
        solver.domains.push_level();

        let status = if solver
            .queue
            .fixpoint(solver.model, &mut solver.domains)
            .is_err()
        {
            self.stats.nodes += 1;
            Status::Complete
        } else {
            match self.node() {
                ControlFlow::Continue(()) => Status::Complete,
                ControlFlow::Break(Stop::Caller) => Status::Stopped,
                ControlFlow::Break(Stop::Budget) => Status::Exhausted,
            }
        };

        self.solver.domains.pop_level();
        status
    }

    /// Searches below the current, propagated domains.
    fn node(&mut self) -> ControlFlow<Stop> {
        self.stats.nodes += 1;
        if self.budget.exhausted(&self.stats) {
            return ControlFlow::Break(Stop::Budget);
        }

        let solver = &*self.solver;
        let Some(var) = solver.var_order.pick(&solver.domains) else {
            self.stats.solutions += 1;
            return (self.on_solution)(&self.solver.domains).map_break(|()| Stop::Caller);
        };

        let solver = &mut *self.solver;
        let values: Vec<u32> =
            solver
                .value_order
                .order(var, solver.model, &mut solver.domains, &mut solver.queue);
        for value in values {
            self.branch(var, value)?;
        }

        ControlFlow::Continue(())
    }

    /// Tries `var = value` and searches below it, then undoes it.
    fn branch(&mut self, var: crate::Var, value: u32) -> ControlFlow<Stop> {
        let solver = &mut *self.solver;
        solver.domains.push_level();

        let result = if assign_and_propagate(solver, var, value).is_ok() {
            self.node()
        } else {
            self.stats.failures += 1;
            ControlFlow::Continue(())
        };

        self.solver.domains.pop_level();
        result
    }
}

/// Fixes `var` to `value` and propagates the consequences.
fn assign_and_propagate<D: Domain>(
    solver: &mut Solver<'_, D>,
    var: crate::Var,
    value: u32,
) -> Result<(), Inconsistent> {
    solver.domains.assign(var, value)?;
    solver
        .queue
        .propagate_changes(solver.model, &mut solver.domains)
}

/// Compile-time check that solvers can move to other threads.
const _: () = {
    /// Accepts only `Send` types.
    const fn sendable<T: Send>() {}
    sendable::<Solver<'static, u64>>();
};

/// Unit tests for the stopping paths of [`Solver`].
#[cfg(test)]
mod tests {
    use core::ops::ControlFlow;

    use super::{Count, Outcome, Solver, Stats, Status};
    use crate::{AllDifferent, Domain, Model};

    /// Three variables over three values, all different: six solutions.
    fn permutations() -> Model<u16> {
        let mut model = Model::new();
        let vars = model.add_vars(3, u16::range(3));
        model.add(AllDifferent::new(vars));

        model
    }

    /// A budget that runs out before the first solution makes `solve` report exhaustion.
    #[test]
    fn solve_reports_exhausted_budget() {
        let model = permutations();
        let report = Solver::new(&model).solve(|stats: &Stats| stats.nodes >= 1);

        assert_eq!(report.outcome, Outcome::Exhausted);
        assert_eq!(report.stats.solutions, 0);
    }

    /// A count stops at its limit and reports a lower bound.
    #[test]
    fn count_stops_at_limit() {
        let model = permutations();
        let report = Solver::new(&model).count(4, |_: &Stats| false);

        assert_eq!(report.outcome, Count::AtLeast(4));
        assert_eq!(report.stats.solutions, 4);
    }

    /// Enumeration reports whether the callback or the search space ended it.
    #[test]
    fn enumeration_status() {
        let model = permutations();
        let mut solver = Solver::new(&model);

        let stopped = solver.for_each_solution(|_: &Stats| false, |_| ControlFlow::Break(()));
        let complete = solver.for_each_solution(|_: &Stats| false, |_| ControlFlow::Continue(()));
        assert_eq!(stopped.outcome, Status::Stopped);
        assert_eq!(
            (complete.outcome, complete.stats.solutions),
            (Status::Complete, 6)
        );
    }
}
