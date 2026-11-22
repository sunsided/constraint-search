//! Parallel search over one shared [`Model`], behind the `parallel` feature (needs `std`).
//!
//! [`ParallelSolver`] splits the search tree into subproblems: it fixes the first few
//! branching variables in every consistent way, and a pool of threads works through those
//! partial assignments, each with its own [`Solver`]. A shared flag stops every thread at the
//! first solution, or when a count reaches its limit (2 for a uniqueness check).
//! [`ParallelSolver::portfolio`] instead races different branching heuristics on the whole
//! problem.
//!
//! ```mermaid
//! flowchart LR
//!     model[Model] --> split[split into partial assignments]
//!     split --> queue[(shared queue)]
//!     queue --> w1[worker: Solver] & w2[worker: Solver] & w3[worker: Solver]
//!     w1 & w2 & w3 -->|solution / limit / budget| stop[shared stop flag]
//! ```

extern crate std;

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;

use search_core::Budget;

use crate::{
    Count, Domain, Inconsistent, Model, Outcome, Progress, Report, Solver, Stats, ValueOrder, Var,
    VarOrder,
};

/// A partial assignment: the subproblem one worker searches.
type Cube = Vec<(Var, u32)>;

/// Backtracking search over a shared model on several threads.
///
/// Each worker thread owns a [`Solver`] and searches subproblems below partial assignments
/// of the first branching variables. Which solution [`ParallelSolver::solve`] returns depends
/// on thread timing; counts are exact whenever the search completes.
///
/// The budget is cloned into every worker and judges that worker's own counters. When any
/// worker's budget runs out, all workers stop and the result is incomplete.
///
/// # Examples
///
/// ```
/// use constraint_search::{AllDifferent, Count, Domain, Model, ParallelSolver, Stats};
///
/// let mut model = Model::<u16>::new();
/// let vars = model.add_vars(6, u16::range(6));
/// model.add(AllDifferent::new(vars));
///
/// let report = ParallelSolver::new(&model, 4).count(u64::MAX, |_: &Stats| false);
/// assert_eq!(report.outcome, Count::Exact(720));
/// ```
pub struct ParallelSolver<'m, D> {
    /// The problem being solved.
    model: &'m Model<D>,
    /// The number of worker threads. At least 1.
    threads: usize,
    /// How workers pick the variable to branch on; also used to split the tree.
    var_order: VarOrder,
    /// How workers order the values of that variable.
    value_order: ValueOrder,
    /// How many subproblems to aim for per thread, so that fast threads take over the work
    /// of slow ones.
    pieces_per_thread: usize,
}

impl<'m, D: Domain> ParallelSolver<'m, D> {
    /// A parallel solver over `model` with `threads` worker threads, branching with
    /// [`VarOrder::Mrv`] and [`ValueOrder::Ascending`].
    ///
    /// # Panics
    ///
    /// Panics if `threads` is 0.
    #[must_use]
    pub fn new(model: &'m Model<D>, threads: usize) -> Self {
        assert!(threads > 0, "a parallel search needs at least one thread");

        Self {
            model,
            threads,
            var_order: VarOrder::default(),
            value_order: ValueOrder::default(),
            pieces_per_thread: 8,
        }
    }

    /// Sets how workers pick the variable to branch on.
    #[must_use]
    pub fn with_var_order(mut self, order: VarOrder) -> Self {
        self.var_order = order;
        self
    }

    /// Sets how workers order the values of the branching variable.
    #[must_use]
    pub fn with_value_order(mut self, order: ValueOrder) -> Self {
        self.value_order = order;
        self
    }

    /// Finds one solution, stopping every thread at the first one found.
    pub fn solve<B: Budget<Stats> + Clone + Send>(&self, budget: B) -> Report<Outcome> {
        let cubes = split(
            self.model,
            self.var_order,
            self.threads * self.pieces_per_thread,
        );
        let shared = Shared::new(Goal::First);
        let orders = vec![(self.var_order, self.value_order); self.threads];
        let stats = run_workers(self.model, &orders, &cubes, &shared, budget, false);

        Report {
            outcome: shared.outcome(),
            stats,
        }
    }

    /// Counts solutions on all threads, stopping every thread once `limit` are found.
    ///
    /// A limit of 2 decides uniqueness. Threads may find a few solutions past the limit
    /// before they see the stop; [`Count::AtLeast`] then reports all of them.
    pub fn count<B: Budget<Stats> + Clone + Send>(&self, limit: u64, budget: B) -> Report<Count> {
        if limit == 0 {
            return Report {
                outcome: Count::AtLeast(0),
                stats: Stats::default(),
            };
        }

        let cubes = split(
            self.model,
            self.var_order,
            self.threads * self.pieces_per_thread,
        );
        let shared = Shared::new(Goal::Count(limit));
        let orders = vec![(self.var_order, self.value_order); self.threads];
        let stats = run_workers(self.model, &orders, &cubes, &shared, budget, false);

        Report {
            outcome: shared.count(),
            stats,
        }
    }

    /// Races one thread per `(VarOrder, ValueOrder)` pair on the whole problem and returns
    /// the first solution any of them finds.
    ///
    /// # Panics
    ///
    /// Panics if `orders` is empty.
    pub fn portfolio<B: Budget<Stats> + Clone + Send>(
        model: &Model<D>,
        orders: &[(VarOrder, ValueOrder)],
        budget: B,
    ) -> Report<Outcome> {
        assert!(
            !orders.is_empty(),
            "a portfolio needs at least one strategy"
        );

        let cubes = vec![Cube::new()];
        let shared = Shared::new(Goal::First);
        let stats = run_workers(model, orders, &cubes, &shared, budget, true);

        Report {
            outcome: shared.outcome(),
            stats,
        }
    }
}

/// What the workers search for.
#[derive(Debug, Clone, Copy)]
enum Goal {
    /// The first solution.
    First,
    /// Solutions, until this many are found.
    Count(u64),
}

/// The state all workers share.
struct Shared {
    /// What the workers search for.
    goal: Goal,
    /// The index of the next subproblem to take.
    next: AtomicUsize,
    /// Set when every worker must stop.
    stop: AtomicBool,
    /// Set when a worker's budget ran out, so the search is incomplete.
    exhausted: AtomicBool,
    /// Solutions found so far, by all workers.
    found: AtomicU64,
    /// The first solution found, for [`Goal::First`].
    solution: Mutex<Option<Vec<u32>>>,
}

impl Shared {
    /// Fresh shared state for `goal`.
    fn new(goal: Goal) -> Self {
        Self {
            goal,
            next: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            exhausted: AtomicBool::new(false),
            found: AtomicU64::new(0),
            solution: Mutex::new(None),
        }
    }

    /// Records the solution in `domains`; returns `true` if the workers should stop.
    fn record<D: Domain>(&self, solver: &Solver<'_, D>) -> bool {
        let found = self.found.fetch_add(1, Ordering::SeqCst) + 1;
        match self.goal {
            Goal::First => {
                let mut solution = self.solution.lock().expect("no worker panics holding it");
                if solution.is_none() {
                    *solution = solver.domains().solution();
                }

                true
            }
            Goal::Count(limit) => found >= limit,
        }
    }

    /// The outcome of a [`Goal::First`] search.
    fn outcome(&self) -> Outcome {
        let solution = self.solution.lock().expect("no worker panics holding it");
        match solution.clone() {
            Some(values) => Outcome::Solved(values),
            None if self.exhausted.load(Ordering::SeqCst) => Outcome::Exhausted,
            None => Outcome::Unsatisfiable,
        }
    }

    /// The outcome of a [`Goal::Count`] search.
    fn count(&self) -> Count {
        let found = self.found.load(Ordering::SeqCst);
        if self.stop.load(Ordering::SeqCst) {
            Count::AtLeast(found)
        } else {
            Count::Exact(found)
        }
    }
}

/// Runs one worker per entry of `orders` over `cubes` and returns their summed counters.
///
/// With `each_takes_all`, every worker searches every cube (a portfolio); otherwise the
/// workers share the cubes out.
fn run_workers<D: Domain, B: Budget<Stats> + Clone + Send>(
    model: &Model<D>,
    orders: &[(VarOrder, ValueOrder)],
    cubes: &[Cube],
    shared: &Shared,
    budget: B,
    each_takes_all: bool,
) -> Stats {
    thread::scope(|scope| {
        // One budget per worker: clones for all but the last, which takes the original.
        let budgets = core::iter::repeat_n(budget, orders.len());
        let workers: Vec<_> = orders
            .iter()
            .zip(budgets)
            .map(|(&(var_order, value_order), budget)| {
                scope.spawn(move || {
                    let solver = Solver::new(model)
                        .with_var_order(var_order)
                        .with_value_order(value_order);
                    let next = AtomicUsize::new(0);
                    let queue = if each_takes_all { &next } else { &shared.next };
                    work(solver, cubes, queue, shared, budget)
                })
            })
            .collect();

        workers
            .into_iter()
            .map(|worker| worker.join().expect("a worker panicked"))
            .fold(Stats::default(), Stats::plus)
    })
}

/// Takes cubes from `queue` and searches below each until the queue runs dry or the shared
/// flag says stop; returns the worker's counters.
fn work<D: Domain>(
    mut solver: Solver<'_, D>,
    cubes: &[Cube],
    queue: &AtomicUsize,
    shared: &Shared,
    mut budget: impl Budget<Stats>,
) -> Stats {
    let mut total = Stats::default();
    while !shared.stop.load(Ordering::SeqCst) {
        let Some(cube) = cubes.get(queue.fetch_add(1, Ordering::SeqCst)) else {
            break;
        };

        if load(&mut solver, cube).is_ok() {
            total = search_cube(&mut solver, shared, &mut budget, total);
        }
    }

    total
}

/// Searches below the loaded cube until it is exhausted or the workers must stop; returns
/// `total` plus the work done.
fn search_cube<D: Domain>(
    solver: &mut Solver<'_, D>,
    shared: &Shared,
    budget: &mut impl Budget<Stats>,
    mut total: Stats,
) -> Stats {
    solver.begin();
    loop {
        let base = total;
        let report = solver.resume(|stats: &Stats| {
            shared.stop.load(Ordering::SeqCst) || budget.exhausted(&base.plus(*stats))
        });
        total = base.plus(report.stats);

        match report.outcome {
            Progress::Solution => {
                if shared.record(solver) {
                    shared.stop.store(true, Ordering::SeqCst);
                    break;
                }
            }
            Progress::Paused => {
                if !shared.stop.swap(true, Ordering::SeqCst) {
                    shared.exhausted.store(true, Ordering::SeqCst);
                }

                break;
            }
            Progress::Complete => break,
        }
    }

    solver.abandon();
    total
}

/// Resets the solver and fixes the assignments of `cube`.
///
/// # Errors
///
/// Returns [`Inconsistent`] if an assignment contradicts the initial domains.
fn load<D: Domain>(solver: &mut Solver<'_, D>, cube: &Cube) -> Result<(), Inconsistent> {
    solver.reset();
    for &(var, value) in cube {
        solver.domains_mut().assign(var, value)?;
    }

    Ok(())
}

/// Splits the search tree into at least `target` partial assignments, where it can, by
/// fixing branching variables breadth first. Inconsistent branches are dropped; an
/// inconsistent model yields none.
fn split<D: Domain>(model: &Model<D>, var_order: VarOrder, target: usize) -> Vec<Cube> {
    let mut solver = Solver::new(model);
    let mut open: VecDeque<Cube> = VecDeque::from([Cube::new()]);
    let mut leaves = Vec::new();

    while open.len() + leaves.len() < target {
        let Some(cube) = open.pop_front() else {
            break;
        };

        if load(&mut solver, &cube)
            .and_then(|()| solver.propagate())
            .is_err()
        {
            continue;
        }

        let Some(var) = var_order.pick(solver.domains()) else {
            leaves.push(cube);
            continue;
        };

        for value in solver.domains()[var].values() {
            let mut child = cube.clone();
            child.push((var, value));
            open.push_back(child);
        }
    }

    open.into_iter().chain(leaves).collect()
}

/// Unit tests for [`ParallelSolver`].
#[cfg(test)]
mod tests {
    use super::{ParallelSolver, split};
    use crate::{AllDifferent, Count, Domain, Model, Outcome, Stats, ValueOrder, VarOrder};

    /// Six variables over six values, all different: 720 solutions.
    fn permutations(n: usize) -> Model<u16> {
        let mut model = Model::new();
        let vars = model.add_vars(n, u16::range(u32::try_from(n).unwrap()));
        model.add(AllDifferent::new(vars));

        model
    }

    /// Counting on several threads matches the exact count, and a limit stops early.
    #[test]
    fn counts_agree_with_sequential() {
        let model = permutations(6);
        for threads in [1, 3, 8] {
            let solver = ParallelSolver::new(&model, threads);
            let report = solver.count(u64::MAX, |_: &Stats| false);
            assert_eq!(report.outcome, Count::Exact(720), "{threads} threads");
            assert_eq!(report.stats.solutions, 720);

            match solver.count(2, |_: &Stats| false).outcome {
                Count::AtLeast(found) => assert!(found >= 2),
                Count::Exact(_) => panic!("720 solutions exceed the limit"),
            }
        }

        let solver = ParallelSolver::new(&model, 2);
        assert_eq!(
            solver.count(0, |_: &Stats| false).outcome,
            Count::AtLeast(0)
        );
    }

    /// Solving returns a valid solution, an unsatisfiable model none.
    #[test]
    fn solve_finds_a_solution_or_none() {
        let model = permutations(5);
        let Outcome::Solved(values) = ParallelSolver::new(&model, 4)
            .solve(|_: &Stats| false)
            .outcome
        else {
            panic!("solvable");
        };

        let mut seen = values.clone();
        seen.sort_unstable();
        assert_eq!(seen, [0, 1, 2, 3, 4]);

        let mut model = Model::<u16>::new();
        let vars = model.add_vars(3, u16::range(2));
        model.add(AllDifferent::new(vars));
        let report = ParallelSolver::new(&model, 3).solve(|_: &Stats| false);
        assert_eq!(report.outcome, Outcome::Unsatisfiable);
        assert_eq!(split(&model, VarOrder::Mrv, 8).len(), 0);
    }

    /// A budget stops all workers, and the portfolio races heuristics to a solution.
    #[test]
    fn budget_and_portfolio() {
        let model = permutations(7);
        let solver = ParallelSolver::new(&model, 2)
            .with_var_order(VarOrder::Input)
            .with_value_order(ValueOrder::Descending);
        let report = solver.count(u64::MAX, |stats: &Stats| stats.nodes >= 50);
        assert!(matches!(report.outcome, Count::AtLeast(_)));
        assert_eq!(solver.solve(|_: &Stats| true).outcome, Outcome::Exhausted);

        let orders = [
            (VarOrder::Mrv, ValueOrder::Ascending),
            (VarOrder::Input, ValueOrder::Lcv),
        ];
        let report = ParallelSolver::portfolio(&model, &orders, |_: &Stats| false);
        assert!(matches!(report.outcome, Outcome::Solved(_)));
    }
}
