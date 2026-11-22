//! The [`Propagator`] trait and the queue that runs propagators to a fixpoint.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Model, Var};

/// A constraint that removes values from variable domains.
///
/// A propagator removes every value it can prove impossible for the variables of its
/// [`scope`](Propagator::scope), and fails when the constraint can no longer be satisfied.
/// It runs again whenever a domain in its scope changes, until no propagator changes anything:
/// the fixpoint.
///
/// Propagators are stateless (`&self`): all state lives in the [`Domains`], which the trail
/// restores on backtrack. They are `Send + Sync`, so one [`Model`] can serve solvers on
/// several threads.
///
/// # Contract
///
/// - **Sound:** never remove a value that is part of a solution of the constraint.
/// - **Checking:** fail when every variable of the scope has one value left and those values
///   violate the constraint. Search relies on this to accept a full assignment.
/// - **Scoped:** only change variables of the scope.
///
/// # Examples
///
/// `x != y`, removing the value of a fixed variable from the other:
///
/// ```
/// use constraint_search::{Domain, Domains, Inconsistent, Propagator, Var};
///
/// struct NotEqual([Var; 2]);
///
/// impl<D: Domain> Propagator<D> for NotEqual {
///     fn scope(&self) -> &[Var] {
///         &self.0
///     }
///
///     fn propagate(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent> {
///         for (fixed, other) in [(self.0[0], self.0[1]), (self.0[1], self.0[0])] {
///             if let Some(value) = domains.value(fixed) {
///                 domains.remove(other, value)?;
///             }
///         }
///
///         Ok(())
///     }
/// }
///
/// let mut domains = Domains::<u16>::new();
/// let x = domains.add(0b1000);
/// let y = domains.add(0b1100);
/// NotEqual([x, y]).propagate(&mut domains)?;
/// assert_eq!(domains[y], 0b0100);
/// # Ok::<(), Inconsistent>(())
/// ```
pub trait Propagator<D: Domain>: Send + Sync {
    /// The variables the constraint is over. A change to any of them wakes the propagator.
    fn scope(&self) -> &[Var];

    /// Removes the values the constraint rules out.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if the constraint cannot be satisfied by the current domains.
    fn propagate(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent>;
}

/// The propagators waiting to run, each at most once. Reused across solves.
#[derive(Debug, Clone, Default)]
pub(crate) struct Queue {
    /// Indices into [`Model::propagators`], in the order they run.
    waiting: VecDeque<usize>,
    /// `queued[p]` is `true` while propagator `p` is in [`Queue::waiting`].
    queued: Vec<bool>,
}

impl Queue {
    /// Runs every propagator of `model`, then whatever they wake, until nothing changes.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if a propagator fails. The queue is cleared either way.
    pub(crate) fn fixpoint<D: Domain>(
        &mut self,
        model: &Model<D>,
        domains: &mut Domains<D>,
    ) -> Result<(), Inconsistent> {
        self.reset(model.propagators().len());
        for index in 0..model.propagators().len() {
            self.push(index);
        }

        domains.drain_modified(|_| {});
        self.run(model, domains)
    }

    /// Runs the propagators watching the variables changed since propagation last ran, then
    /// whatever they wake, until nothing changes.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if a propagator fails. The queue is cleared either way.
    pub(crate) fn propagate_changes<D: Domain>(
        &mut self,
        model: &Model<D>,
        domains: &mut Domains<D>,
    ) -> Result<(), Inconsistent> {
        self.reset(model.propagators().len());
        self.wake(model, domains);

        self.run(model, domains)
    }

    /// Pops and runs propagators until the queue is empty or one fails.
    fn run<D: Domain>(
        &mut self,
        model: &Model<D>,
        domains: &mut Domains<D>,
    ) -> Result<(), Inconsistent> {
        while let Some(index) = self.waiting.pop_front() {
            self.queued[index] = false;

            if let Err(conflict) = model.propagators()[index].propagate(domains) {
                self.clear();
                domains.drain_modified(|_| {});
                return Err(conflict);
            }

            self.wake(model, domains);
        }

        Ok(())
    }

    /// Queues the watchers of every variable changed since the last wake.
    fn wake<D: Domain>(&mut self, model: &Model<D>, domains: &mut Domains<D>) {
        domains.drain_modified(|var| {
            for &index in model.watchers(var) {
                self.push(index);
            }
        });
    }

    /// Queues propagator `index` unless it is already waiting.
    fn push(&mut self, index: usize) {
        if !self.queued[index] {
            self.queued[index] = true;
            self.waiting.push_back(index);
        }
    }

    /// Empties the queue and sizes it for `propagators` propagators.
    fn reset(&mut self, propagators: usize) {
        self.clear();
        self.queued.resize(propagators, false);
    }

    /// Empties the queue.
    fn clear(&mut self) {
        for index in self.waiting.drain(..) {
            self.queued[index] = false;
        }
    }
}
