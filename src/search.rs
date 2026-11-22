//! Backtracking search: the [`Solver`] that finds, counts and enumerates solutions of a
//! [`Model`].
//!
//! Each node picks an unfixed variable, tries its values in turn and propagates after each
//! choice; a conflict undoes the choice through the trail.

mod order;
mod walk;

use alloc::vec::Vec;
use core::ops::ControlFlow;

use search_core::Budget;

use crate::propagator::Engine;
use crate::{Domain, Domains, Inconsistent, Model};
pub use order::{ValueOrder, VarOrder};
pub use walk::Progress;
use walk::{Frame, Phase};

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
///
/// `T` is the solution: the value of every variable for a [`Solver`], the chosen rows for an
/// [`ExactCover`](crate::ExactCover).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T = Vec<u32>> {
    /// A solution: for a [`Solver`], the value of every variable in index order.
    Solved(T),
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
/// The solver owns the current domains, their trail, the propagation queue and every
/// propagator's scratch memory, and keeps their allocations from one solve to the next:
/// once warmed up, [`Solver::count`] and [`Solver::for_each_solution`] allocate nothing, and
/// [`Solver::solve`] allocates only the solution it returns. [`Solver::reset`] returns to the model's
/// initial domains without reallocating. Search leaves the domains as it found them, so a
/// caller can fix some variables ([`Solver::domains_mut`]) and search many times from there.
///
/// [`Solver::solve`], [`Solver::count`] and [`Solver::for_each_solution`] run one search to
/// its end or budget. For time slicing, [`Solver::begin`] starts a search that
/// [`Solver::resume`] continues a little at a time, pausing at each solution or when its
/// budget runs out.
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
    /// The propagation queue and each propagator's scratch memory.
    engine: Engine,
    /// The values still to try at each open search node, stacked: a node's values sit above
    /// its parent's and are popped when the node returns.
    values: Vec<u32>,
    /// Scratch for [`ValueOrder::Lcv`]: how many values each candidate leaves, and the value.
    scores: Vec<(u64, u32)>,
    /// The open nodes of the search in progress, root first.
    frames: Vec<Frame>,
    /// The next step of the search in progress.
    phase: Phase,
    /// The trail level of the domains when the search began.
    base_level: usize,
    /// A value per variable to try first when branching on it; empty for none.
    hint: Vec<u32>,
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
            engine: Engine::new(model),
            values: Vec::new(),
            scores: Vec::new(),
            frames: Vec::new(),
            phase: Phase::Idle,
            base_level: 0,
            hint: Vec::new(),
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

    /// Tries `values[i]` first whenever the search branches on variable `i`, then the other
    /// values in the [`ValueOrder`]. A hint value no longer in the domain is skipped.
    ///
    /// Give a known solution as the hint to find it first: a generator that digs clues out of
    /// a solved puzzle checks uniqueness with [`Solver::count`] and a limit of 2, and the hint
    /// turns finding the first solution into a straight walk, leaving only the search for a
    /// second. The hint stays set across searches and [`Solver::reset`].
    ///
    /// # Panics
    ///
    /// Panics if `values` does not hold exactly one value per variable of the model.
    pub fn set_hint(&mut self, values: &[u32]) {
        assert_eq!(
            values.len(),
            self.model.num_vars(),
            "a hint needs one value per variable"
        );

        self.hint.clear();
        self.hint.extend_from_slice(values);
    }

    /// Removes the hint set by [`Solver::set_hint`].
    pub fn clear_hint(&mut self) {
        self.hint.clear();
    }

    /// Returns to the model's initial domains, keeping every allocation. Abandons a search in
    /// progress.
    pub fn reset(&mut self) {
        self.abandon();
        self.domains.reset_to(self.model.initial_domains());
    }

    /// The current domains.
    #[must_use]
    pub fn domains(&self) -> &Domains<D> {
        &self.domains
    }

    /// The current domains, to fix or restrict variables before a search. Abandons a search
    /// in progress, which owns the domains until it completes.
    pub fn domains_mut(&mut self) -> &mut Domains<D> {
        self.abandon();
        &mut self.domains
    }

    /// Runs every propagator to a fixpoint on the current domains, keeping the result.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if the constraints cannot be satisfied. The domains are then
    /// partly propagated; [`Solver::reset`] starts over.
    pub fn propagate(&mut self) -> Result<(), Inconsistent> {
        self.engine.fixpoint(self.model, &mut self.domains)
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
    /// `Count::AtLeast(2)` more than one. A limit of 0 searches nothing and reports
    /// `Count::AtLeast(0)`.
    pub fn count(&mut self, limit: u64, budget: impl Budget<Stats>) -> Report<Count> {
        if limit == 0 {
            return Report {
                outcome: Count::AtLeast(0),
                stats: Stats::default(),
            };
        }

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
        mut budget: impl Budget<Stats>,
        mut on_solution: impl FnMut(&Domains<D>) -> ControlFlow<()>,
    ) -> Report<Status> {
        let mut stats = Stats::default();
        self.begin();

        let outcome = loop {
            match self.run(&mut budget, &mut stats) {
                Progress::Solution => {
                    if on_solution(&self.domains).is_break() {
                        break Status::Stopped;
                    }
                }
                Progress::Paused => break Status::Exhausted,
                Progress::Complete => break Status::Complete,
            }
        };

        self.abandon();
        Report { outcome, stats }
    }
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

    use super::{Count, Outcome, Progress, Solver, Stats, Status};
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

    /// A one-node budget still examines that node, so an already solved problem is solved.
    #[test]
    fn budget_counts_examined_nodes() {
        let mut model = Model::<u16>::new();
        model.add_var(0b100);
        let report = Solver::new(&model).solve(|stats: &Stats| stats.nodes >= 1);

        assert_eq!(report.outcome, Outcome::Solved(alloc::vec![2]));
        assert_eq!(report.stats.nodes, 1);
    }

    /// A zero limit searches nothing.
    #[test]
    fn zero_limit_searches_nothing() {
        let model = permutations();
        let report = Solver::new(&model).count(0, |_: &Stats| false);

        assert_eq!(report.outcome, Count::AtLeast(0));
        assert_eq!(report.stats.nodes, 0);
    }

    /// A count stops at its limit and reports a lower bound.
    #[test]
    fn count_stops_at_limit() {
        let model = permutations();
        let report = Solver::new(&model).count(4, |_: &Stats| false);

        assert_eq!(report.outcome, Count::AtLeast(4));
        assert_eq!(report.stats.solutions, 4);
    }

    /// Abandoning, resetting or editing the domains ends a search and restores the domains.
    #[test]
    fn abandoning_restores_domains() {
        let model = permutations();
        let mut solver = Solver::new(&model);
        assert_eq!(solver.resume(|_: &Stats| false).outcome, Progress::Complete);

        solver.begin();
        assert_eq!(solver.resume(|_: &Stats| false).outcome, Progress::Solution);
        assert!(solver.is_searching());
        assert!(solver.domains().is_solved());

        solver.abandon();
        assert!(!solver.is_searching());
        assert!(!solver.domains().is_solved());

        solver.begin();
        let _ = solver.resume(|_: &Stats| false);
        let _ = solver.domains_mut();
        assert!(!solver.is_searching());
        assert_eq!(solver.domains().level(), 0);
    }

    /// A hint leads the search straight to the hinted solution, without changing counts.
    #[test]
    fn hint_is_tried_first() {
        let model = permutations();
        let mut solver = Solver::new(&model);
        solver.set_hint(&[2, 0, 1]);

        let report = solver.solve(|_: &Stats| false);
        assert_eq!(report.outcome, Outcome::Solved(alloc::vec![2, 0, 1]));
        assert_eq!(report.stats.failures, 0);
        assert_eq!(
            solver.count(u64::MAX, |_: &Stats| false).outcome,
            Count::Exact(6)
        );

        solver.clear_hint();
        assert_eq!(
            solver.solve(|_: &Stats| false).outcome,
            Outcome::Solved(alloc::vec![0, 1, 2])
        );
    }

    /// A hint must cover every variable.
    #[test]
    #[should_panic(expected = "one value per variable")]
    fn short_hint_panics() {
        let model = permutations();
        Solver::new(&model).set_hint(&[0]);
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
