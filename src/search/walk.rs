//! The resumable depth-first walk: an explicit stack of choice points kept in the [`Solver`],
//! so a search can stop at a solution or on its budget and continue later.
//!
//! Every choice pushes a trail level, which the walk pops when it backs out of that choice.
//! The values still to try sit on [`Solver::values`], one slice per open node.

use search_core::Budget;

use super::{Report, Solver, Stats};
use crate::{Domain, Var};

/// Where a resumable search stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// The domains hold a solution, one value per variable. Resuming continues after it.
    Solution,
    /// The budget stopped the search. Resuming continues where it stopped.
    Paused,
    /// The search space is exhausted, and the domains are as they were at
    /// [`Solver::begin`]. Also returned when no search is in progress.
    Complete,
}

/// The next step of the walk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Phase {
    /// No search is in progress.
    #[default]
    Idle,
    /// Propagate the starting domains.
    Start,
    /// Enter the node the current domains describe.
    Expand,
    /// Try the next value of the innermost open node.
    Advance,
    /// Back out of the current node into its parent.
    Retreat,
}

/// An open search node: the variable it branches on and its values still to try.
#[derive(Debug, Clone, Copy)]
pub(super) struct Frame {
    /// The variable this node branches on.
    var: Var,
    /// Where the node's values start on [`Solver::values`].
    start: usize,
    /// The position of the next value to try.
    next: usize,
    /// One past the node's last value.
    end: usize,
}

impl<D: Domain> Solver<'_, D> {
    /// Starts a resumable search from the current domains, abandoning any search in progress.
    ///
    /// Drive it with [`Solver::resume`]. Until it completes or is abandoned, the domains
    /// belong to the search: [`Solver::domains_mut`] and [`Solver::reset`] abandon it.
    ///
    /// # Examples
    ///
    /// Enumerate solutions a few nodes at a time, as a game might spread work across frames:
    ///
    /// ```
    /// use constraint_search::{AllDifferent, Domain, Model, Progress, Solver, Stats};
    ///
    /// let mut model = Model::<u16>::new();
    /// let vars = model.add_vars(4, u16::range(4));
    /// model.add(AllDifferent::new(vars));
    ///
    /// let mut solver = Solver::new(&model);
    /// solver.begin();
    /// let (mut solutions, mut slices) = (0, 0);
    /// loop {
    ///     slices += 1;
    ///     match solver.resume(|stats: &Stats| stats.nodes >= 3).outcome {
    ///         Progress::Solution => solutions += 1,
    ///         Progress::Paused => {}
    ///         Progress::Complete => break,
    ///     }
    /// }
    ///
    /// assert_eq!(solutions, 24);
    /// assert!(slices > solutions);
    /// ```
    pub fn begin(&mut self) {
        self.abandon();
        self.base_level = self.domains.level();
        self.phase = Phase::Start;
    }

    /// Continues the search until the next solution, until `budget` stops it, or until the
    /// search space is exhausted. The budget and the returned counters cover this call only.
    pub fn resume(&mut self, mut budget: impl Budget<Stats>) -> Report<Progress> {
        let mut stats = Stats::default();
        let outcome = self.run(&mut budget, &mut stats);

        Report { outcome, stats }
    }

    /// Stops the search in progress, if any, and restores the domains to how they were at
    /// [`Solver::begin`].
    pub fn abandon(&mut self) {
        if self.phase == Phase::Idle {
            return;
        }

        while self.domains.level() > self.base_level {
            self.domains.pop_level();
        }

        self.frames.clear();
        self.values.clear();
        self.phase = Phase::Idle;
    }

    /// Returns `true` while a search started with [`Solver::begin`] has not completed or been
    /// abandoned.
    #[must_use]
    pub fn is_searching(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Walks until a solution, the budget or the end, adding the work done to `stats`.
    pub(super) fn run(&mut self, budget: &mut impl Budget<Stats>, stats: &mut Stats) -> Progress {
        loop {
            let progress = match self.phase {
                Phase::Idle => return Progress::Complete,
                Phase::Start => self.start(stats),
                Phase::Expand => self.expand(budget, stats),
                Phase::Advance => self.advance(stats),
                Phase::Retreat => self.retreat(),
            };

            if let Some(progress) = progress {
                return progress;
            }
        }
    }

    /// Opens the root level and propagates the starting domains.
    fn start(&mut self, stats: &mut Stats) -> Option<Progress> {
        self.domains.push_level();
        if self.engine.fixpoint(self.model, &mut self.domains).is_err() {
            stats.nodes += 1;
            return Some(self.finish());
        }

        self.phase = Phase::Expand;
        None
    }

    /// Enters the current node: reports a solution, or opens a frame over the values of the
    /// variable to branch on.
    fn expand(&mut self, budget: &mut impl Budget<Stats>, stats: &mut Stats) -> Option<Progress> {
        // Consult the budget before charging the node, so `nodes` counts nodes examined.
        if budget.exhausted(stats) {
            return Some(Progress::Paused);
        }

        stats.nodes += 1;
        let Some(var) = self.var_order.pick(&self.domains) else {
            stats.solutions += 1;
            self.phase = Phase::Retreat;
            return Some(Progress::Solution);
        };

        let start = self.values.len();
        stats.failures += self.value_order.order(
            var,
            self.model,
            &mut self.domains,
            &mut self.engine,
            &mut self.values,
            &mut self.scores,
        );
        self.frames.push(Frame {
            var,
            start,
            next: start,
            end: self.values.len(),
        });

        self.phase = Phase::Advance;
        None
    }

    /// Tries the next value of the innermost node; a refuted value is undone at once.
    fn advance(&mut self, stats: &mut Stats) -> Option<Progress> {
        let frame = self.frames.last_mut().expect("advance needs an open node");
        if frame.next == frame.end {
            self.values.truncate(frame.start);
            self.frames.pop();
            self.phase = Phase::Retreat;
            return None;
        }

        let (var, value) = (frame.var, self.values[frame.next]);
        frame.next += 1;

        self.domains.push_level();
        let propagated = self
            .domains
            .assign(var, value)
            .and_then(|_| self.engine.propagate_changes(self.model, &mut self.domains));
        if propagated.is_ok() {
            self.phase = Phase::Expand;
        } else {
            stats.failures += 1;
            self.domains.pop_level();
        }

        None
    }

    /// Backs out of the current node: undoes the choice that led to it, or finishes the
    /// search at the root.
    fn retreat(&mut self) -> Option<Progress> {
        if self.frames.is_empty() {
            return Some(self.finish());
        }

        self.domains.pop_level();
        self.phase = Phase::Advance;
        None
    }

    /// Ends the search, restoring the domains to how they were at [`Solver::begin`].
    fn finish(&mut self) -> Progress {
        self.abandon();
        Progress::Complete
    }
}
