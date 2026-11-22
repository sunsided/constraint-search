//! The variable store: [`Domains`] with a trail, so that search can undo changes on backtrack.

use alloc::vec::Vec;
use core::fmt;
use core::ops::Index;

use crate::Domain;

/// A variable: a dense index into [`Domains`] and [`Model`](crate::Model).
///
/// Variables are numbered from zero in the order they were added, so a caller can keep its
/// own layout (cell `i` of a grid is `Var::new(i)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Var(
    /// The position in the store.
    u32,
);

impl Var {
    /// The variable with the given index.
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The position of the variable in the store.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A constraint cannot be satisfied any more: some domain would become empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Inconsistent;

impl fmt::Display for Inconsistent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the constraints cannot be satisfied")
    }
}

impl core::error::Error for Inconsistent {}

/// The current domain of every variable, with a trail of earlier domains.
///
/// Every change records the domain it replaced. [`Domains::push_level`] marks the trail and
/// [`Domains::pop_level`] restores every domain changed since the mark, which is how search
/// backtracks. Domains never become empty: a change that would empty one fails with
/// [`Inconsistent`] and leaves the domain as it was.
///
/// # Examples
///
/// ```
/// use constraint_search::{Domains, Var};
///
/// let mut domains = Domains::<u16>::new();
/// let x = domains.add(0b111);
///
/// domains.push_level();
/// domains.assign(x, 2)?;
/// assert_eq!(domains.value(x), Some(2));
///
/// domains.pop_level();
/// assert_eq!(domains[x], 0b111);
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
#[derive(Debug, Clone)]
pub struct Domains<D> {
    /// The current domain of each variable, indexed by [`Var::index`]. Never empty.
    current: Vec<D>,
    /// The domains replaced by changes, oldest first.
    trail: Vec<(Var, D)>,
    /// The trail length at each [`Domains::push_level`], innermost last.
    levels: Vec<usize>,
    /// Variables changed since propagation last looked, each listed once.
    modified: Vec<Var>,
    /// `pending[i]` is `true` while variable `i` is in [`Domains::modified`].
    pending: Vec<bool>,
}

impl<D: Domain> Domains<D> {
    /// An empty store.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: Vec::new(),
            trail: Vec::new(),
            levels: Vec::new(),
            modified: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Adds a variable with the given initial domain.
    ///
    /// # Panics
    ///
    /// Panics if `domain` is empty or if the store already holds `u32::MAX` variables.
    pub fn add(&mut self, domain: D) -> Var {
        assert!(!domain.is_empty(), "a variable needs at least one value");

        let var = Var(u32::try_from(self.current.len()).expect("fewer than u32::MAX variables"));
        self.current.push(domain);
        self.pending.push(false);

        var
    }

    /// Replaces the contents with `initial`, keeping the allocations for reuse.
    pub(crate) fn reset_to(&mut self, initial: &[D]) {
        self.current.clear();
        self.current.extend_from_slice(initial);
        self.trail.clear();
        self.levels.clear();
        self.modified.clear();
        self.pending.clear();
        self.pending.resize(initial.len(), false);
    }

    /// The number of variables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.current.len()
    }

    /// Returns `true` if the store holds no variable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    /// All variables, in index order.
    ///
    /// # Panics
    ///
    /// Never: [`Domains::add`] keeps the count below `u32::MAX`.
    pub fn vars(&self) -> impl ExactSizeIterator<Item = Var> + use<D> {
        (0..u32::try_from(self.current.len()).expect("fewer than u32::MAX variables")).map(Var)
    }

    /// The only value left for `var`, if its domain holds exactly one.
    #[must_use]
    pub fn value(&self, var: Var) -> Option<u32> {
        self[var].value()
    }

    /// Returns `true` if every variable has exactly one value left.
    #[must_use]
    pub fn is_solved(&self) -> bool {
        self.current.iter().all(|domain| domain.len() == 1)
    }

    /// The value of every variable in index order, if every domain holds exactly one.
    #[must_use]
    pub fn solution(&self) -> Option<Vec<u32>> {
        self.current.iter().map(Domain::value).collect()
    }

    /// Restricts `var` to the values in `allowed`; returns `true` if its domain shrank.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if no value of the domain is in `allowed`. The domain is then
    /// left unchanged.
    pub fn restrict(&mut self, var: Var, allowed: D) -> Result<bool, Inconsistent> {
        let current = self[var];
        let narrowed = current.intersection(allowed);
        if narrowed == current {
            return Ok(false);
        }

        if narrowed.is_empty() {
            return Err(Inconsistent);
        }

        self.replace(var, current, narrowed);
        Ok(true)
    }

    /// Fixes `var` to `value`; returns `true` if its domain shrank.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if `value` is not in the domain of `var`.
    pub fn assign(&mut self, var: Var, value: u32) -> Result<bool, Inconsistent> {
        if !self[var].contains(value) {
            return Err(Inconsistent);
        }

        self.restrict(var, D::singleton(value))
    }

    /// Removes `value` from `var`; returns `true` if the value was there.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if `value` is the last value of `var`.
    pub fn remove(&mut self, var: Var, value: u32) -> Result<bool, Inconsistent> {
        let mut allowed = self[var];
        allowed.remove(value);

        self.restrict(var, allowed)
    }

    /// Removes every value in `values` from `var`; returns `true` if its domain shrank.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if that would leave `var` without a value.
    pub fn remove_all(&mut self, var: Var, values: D) -> Result<bool, Inconsistent> {
        self.restrict(var, self[var].difference(values))
    }

    /// Marks the trail, so that [`Domains::pop_level`] can return to the current domains.
    pub fn push_level(&mut self) {
        self.levels.push(self.trail.len());
    }

    /// Restores every domain changed since the matching [`Domains::push_level`].
    ///
    /// # Panics
    ///
    /// Panics if no level was pushed.
    pub fn pop_level(&mut self) {
        let mark = self.levels.pop().expect("pop_level without push_level");
        for (var, domain) in self.trail.drain(mark..).rev() {
            self.current[var.index()] = domain;
        }
    }

    /// The number of levels pushed and not yet popped.
    #[must_use]
    pub fn level(&self) -> usize {
        self.levels.len()
    }

    /// The current trail length, to compare with [`Domains::removed_since`] later.
    pub(crate) fn trail_len(&self) -> usize {
        self.trail.len()
    }

    /// The values removed from each variable since the trail had length `mark`, in the order
    /// the variables were first changed.
    pub(crate) fn removed_since(&self, mark: usize) -> Vec<(Var, D)> {
        let mut removed: Vec<(Var, D)> = Vec::new();
        for &(var, before) in &self.trail[mark..] {
            if removed.iter().all(|(seen, _)| *seen != var) {
                removed.push((var, before.difference(self[var])));
            }
        }

        removed
    }

    /// Hands the variables changed since the last call to `wake`, and forgets them.
    pub(crate) fn drain_modified(&mut self, mut wake: impl FnMut(Var)) {
        for var in self.modified.drain(..) {
            self.pending[var.index()] = false;
            wake(var);
        }
    }

    /// Sets `var` from `old` to `new`, recording the change on the trail.
    fn replace(&mut self, var: Var, old: D, new: D) {
        self.trail.push((var, old));
        self.current[var.index()] = new;

        if !self.pending[var.index()] {
            self.pending[var.index()] = true;
            self.modified.push(var);
        }
    }
}

impl<D: Domain> Default for Domains<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D> Index<Var> for Domains<D> {
    type Output = D;

    fn index(&self, var: Var) -> &D {
        &self.current[var.index()]
    }
}

/// Unit tests for [`Domains`].
#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::{Domains, Inconsistent, Var};

    /// A store with two variables over `{0, 1, 2}`.
    fn two_vars() -> (Domains<u16>, Var, Var) {
        let mut domains = Domains::new();
        let x = domains.add(0b111);
        let y = domains.add(0b111);

        (domains, x, y)
    }

    /// Nested levels undo exactly their own changes.
    #[test]
    fn levels_undo_in_order() {
        let (mut domains, x, y) = two_vars();

        domains.push_level();
        domains.remove(x, 0).unwrap();
        domains.push_level();
        domains.assign(y, 2).unwrap();
        domains.remove(x, 1).unwrap();

        domains.pop_level();
        assert_eq!((domains[x], domains[y]), (0b110, 0b111));
        domains.pop_level();
        assert_eq!((domains[x], domains[y]), (0b111, 0b111));
    }

    /// Emptying a domain fails and leaves it, and the trail, untouched.
    #[test]
    fn emptying_fails_without_change() {
        let (mut domains, x, _) = two_vars();

        assert_eq!(domains.remove_all(x, 0b111), Err(Inconsistent));
        assert_eq!(domains.assign(x, 7), Err(Inconsistent));
        assert_eq!(domains[x], 0b111);
        assert_eq!(domains.trail_len(), 0);
    }

    /// The removal log nets out repeated changes to one variable.
    #[test]
    fn removed_since_nets_changes() {
        let (mut domains, x, y) = two_vars();
        domains.remove(y, 0).unwrap();

        let mark = domains.trail_len();
        domains.remove(x, 0).unwrap();
        domains.remove(y, 1).unwrap();
        domains.remove(x, 1).unwrap();

        assert_eq!(domains.removed_since(mark), vec![(x, 0b011), (y, 0b010)]);
    }

    /// Each changed variable is reported once until drained.
    #[test]
    fn modified_lists_each_var_once() {
        let (mut domains, x, _) = two_vars();
        domains.remove(x, 0).unwrap();
        domains.remove(x, 1).unwrap();

        let mut woken = vec![];
        domains.drain_modified(|var| woken.push(var));
        assert_eq!(woken, vec![x]);

        domains.drain_modified(|var| woken.push(var));
        assert_eq!(woken, vec![x]);
    }
}
