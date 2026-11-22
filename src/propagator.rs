//! The [`Propagator`] trait and the engine that runs propagators to a fixpoint.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::any::Any;

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
/// # Scratch memory
///
/// Working memory goes in [`Propagator::Scratch`]. Each [`Solver`](crate::Solver) creates one
/// scratch value per propagator and passes it to every call, so buffers allocated on the
/// first calls are reused afterwards and a warmed-up solve allocates nothing. Scratch is not
/// restored on backtrack: treat its contents as garbage at the start of each call. A
/// propagator that needs none uses `()`.
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
///     type Scratch = ();
///
///     fn scope(&self) -> &[Var] {
///         &self.0
///     }
///
///     fn propagate(&self, domains: &mut Domains<D>, (): &mut ()) -> Result<(), Inconsistent> {
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
/// NotEqual([x, y]).propagate(&mut domains, &mut ())?;
/// assert_eq!(domains[y], 0b0100);
/// # Ok::<(), Inconsistent>(())
/// ```
pub trait Propagator<D: Domain>: Send + Sync {
    /// Working memory reused across calls; `()` for none. See the trait docs.
    type Scratch: Default + Send + 'static;

    /// The variables the constraint is over. A change to any of them wakes the propagator.
    fn scope(&self) -> &[Var];

    /// Removes the values the constraint rules out, using `scratch` as working memory.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if the constraint cannot be satisfied by the current domains.
    fn propagate(
        &self,
        domains: &mut Domains<D>,
        scratch: &mut Self::Scratch,
    ) -> Result<(), Inconsistent>;
}

/// Scratch memory of one propagator, with its type erased.
pub(crate) type AnyScratch = Box<dyn Any + Send>;

/// A [`Propagator`] with its scratch type erased, so a [`Model`] can hold different kinds.
pub(crate) trait Erased<D: Domain>: Send + Sync {
    /// A fresh scratch value of the propagator's type.
    fn new_scratch(&self) -> AnyScratch;

    /// See [`Propagator::propagate`]; `scratch` must come from [`Erased::new_scratch`] of
    /// the same propagator.
    fn propagate(
        &self,
        domains: &mut Domains<D>,
        scratch: &mut AnyScratch,
    ) -> Result<(), Inconsistent>;
}

impl<D: Domain, P: Propagator<D>> Erased<D> for P {
    fn new_scratch(&self) -> AnyScratch {
        Box::new(P::Scratch::default())
    }

    fn propagate(
        &self,
        domains: &mut Domains<D>,
        scratch: &mut AnyScratch,
    ) -> Result<(), Inconsistent> {
        let scratch = scratch
            .downcast_mut::<P::Scratch>()
            .expect("the engine pairs each propagator with its own scratch");

        Propagator::propagate(self, domains, scratch)
    }
}

/// Runs a model's propagators to a fixpoint: the waiting queue and each propagator's
/// scratch memory. Reused across solves.
pub(crate) struct Engine {
    /// Indices into the model's propagators, in the order they run.
    waiting: VecDeque<usize>,
    /// `queued[p]` is `true` while propagator `p` is in [`Engine::waiting`].
    queued: Vec<bool>,
    /// The scratch memory of each propagator, by index.
    scratch: Vec<AnyScratch>,
}

impl Engine {
    /// An engine for `model`, with one scratch value per propagator.
    pub(crate) fn new<D: Domain>(model: &Model<D>) -> Self {
        let propagators = model.erased();

        Self {
            waiting: VecDeque::with_capacity(propagators.len()),
            queued: alloc::vec![false; propagators.len()],
            scratch: propagators.iter().map(|p| p.new_scratch()).collect(),
        }
    }

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
        self.clear();
        for index in 0..model.erased().len() {
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
        self.clear();
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

            let propagator = &model.erased()[index];
            if let Err(conflict) = propagator.propagate(domains, &mut self.scratch[index]) {
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

    /// Empties the queue.
    fn clear(&mut self) {
        for index in self.waiting.drain(..) {
            self.queued[index] = false;
        }
    }
}
