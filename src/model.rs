//! The [`Model`]: variables with their initial domains, and the propagators over them.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::{Domain, Propagator, Var};

/// A constraint problem: variables, their initial domains and the propagators over them.
///
/// A model is built once and then only read. It is `Send + Sync`, so one model can serve
/// any number of [`Solver`](crate::Solver)s, on one thread or several.
///
/// # Examples
///
/// ```
/// use constraint_search::{AllDifferent, Domain, Model};
///
/// let mut model = Model::<u16>::new();
/// let vars = model.add_vars(3, u16::range(3));
/// model.add(AllDifferent::new(vars));
/// assert_eq!(model.num_vars(), 3);
/// ```
pub struct Model<D> {
    /// The domain each variable starts with, indexed by [`Var::index`]. Never empty.
    initial: Vec<D>,
    /// The constraints, in the order they were added.
    propagators: Vec<Box<dyn Propagator<D>>>,
    /// For each variable, the indices of the propagators whose scope holds it.
    watchers: Vec<Vec<usize>>,
}

impl<D: Domain> Model<D> {
    /// A model without variables or constraints.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            initial: Vec::new(),
            propagators: Vec::new(),
            watchers: Vec::new(),
        }
    }

    /// Adds a variable that may take any value of `domain`.
    ///
    /// # Panics
    ///
    /// Panics if `domain` is empty or if the model already has `u32::MAX` variables.
    pub fn add_var(&mut self, domain: D) -> Var {
        assert!(!domain.is_empty(), "a variable needs at least one value");

        let var =
            Var::new(u32::try_from(self.initial.len()).expect("fewer than u32::MAX variables"));
        self.initial.push(domain);
        self.watchers.push(Vec::new());

        var
    }

    /// Adds `count` variables that may each take any value of `domain`.
    ///
    /// # Panics
    ///
    /// Panics if `domain` is empty or if the model would exceed `u32::MAX` variables.
    pub fn add_vars(&mut self, count: usize, domain: D) -> Vec<Var> {
        (0..count).map(|_| self.add_var(domain)).collect()
    }

    /// Adds a constraint.
    ///
    /// # Panics
    ///
    /// Panics if the scope of `propagator` holds a variable that is not in the model.
    pub fn add(&mut self, propagator: impl Propagator<D> + 'static) -> &mut Self {
        let index = self.propagators.len();
        for var in propagator.scope() {
            let watchers = self
                .watchers
                .get_mut(var.index())
                .expect("the propagator's scope only holds variables of this model");
            if watchers.last() != Some(&index) {
                watchers.push(index);
            }
        }

        self.propagators.push(Box::new(propagator));
        self
    }

    /// The number of variables.
    #[must_use]
    pub fn num_vars(&self) -> usize {
        self.initial.len()
    }

    /// The domain each variable starts with, indexed by [`Var::index`].
    #[must_use]
    pub fn initial_domains(&self) -> &[D] {
        &self.initial
    }

    /// The constraints, in the order they were added.
    #[must_use]
    pub fn propagators(&self) -> &[Box<dyn Propagator<D>>] {
        &self.propagators
    }

    /// The indices of the propagators whose scope holds `var`.
    pub(crate) fn watchers(&self, var: Var) -> &[usize] {
        &self.watchers[var.index()]
    }
}

impl<D: Domain> Default for Model<D> {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile-time check that a model can be shared between threads.
const _: () = {
    /// Accepts only `Send + Sync` types.
    const fn shareable<T: Send + Sync>() {}
    shareable::<Model<u64>>();
};
