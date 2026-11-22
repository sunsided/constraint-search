//! [`Linear`]: a weighted sum of variables compared with a constant.

use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Propagator, Var};

/// How the sum of a [`Linear`] constraint compares with its right-hand side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    /// The sum is at most the right-hand side.
    AtMost,
    /// The sum equals the right-hand side.
    Equal,
    /// The sum is at least the right-hand side.
    AtLeast,
}

/// `a₁·x₁ + a₂·x₂ + … (≤ | = | ≥) b`, where each value is read as the integer it is.
///
/// This covers Kakuro and Killer-Sudoku cages (sums of digits), Minesweeper (how many of
/// the neighbouring cells, each 0 or 1, are mines), Battleships row and column counts, card
/// counts per hand, and cryptarithms such as SEND + MORE = MONEY (one equation with place
/// values as coefficients).
///
/// Propagation reasons on bounds: a value stays only if it can be completed with the smallest
/// (for `≤`) or largest (for `≥`) contributions the other terms can still make. It fails once
/// those bounds cannot meet the right-hand side, so a full assignment is always checked. It
/// is sound but, for `=`, not complete: a value whose completions all fall between the bounds
/// may stay. Coefficients may be negative; sums are computed in `i128`, so they never
/// overflow.
///
/// # Examples
///
/// A Killer-Sudoku cage of two cells summing to 15, digits 1 to 9:
///
/// ```
/// use constraint_search::{Domain, Domains, Linear, Propagator, Relation};
///
/// let mut domains = Domains::<u16>::new();
/// let digits = u16::range(10).difference(u16::singleton(0));
/// let a = domains.add(digits);
/// let b = domains.add(digits);
///
/// let cage = Linear::sum([a, b], Relation::Equal, 15);
/// cage.propagate(&mut domains, &mut Vec::new())?;
/// assert!(domains[a].values().eq(6..=9));
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linear {
    /// The variables, in term order.
    vars: Vec<Var>,
    /// The coefficient of each variable, in term order.
    coefficients: Vec<i64>,
    /// How the sum compares with [`Linear::rhs`].
    relation: Relation,
    /// The right-hand side.
    rhs: i64,
}

impl Linear {
    /// The constraint `Σ coefficient · var (relation) rhs` over the given terms.
    ///
    /// A variable may appear in several terms; propagation then treats the terms
    /// independently, which stays sound.
    #[must_use]
    pub fn new(terms: impl IntoIterator<Item = (i64, Var)>, relation: Relation, rhs: i64) -> Self {
        let (coefficients, vars) = terms.into_iter().unzip();

        Self {
            vars,
            coefficients,
            relation,
            rhs,
        }
    }

    /// The constraint `Σ var (relation) rhs`: every coefficient is 1.
    #[must_use]
    pub fn sum(vars: impl IntoIterator<Item = Var>, relation: Relation, rhs: i64) -> Self {
        Self::new(vars.into_iter().map(|var| (1, var)), relation, rhs)
    }

    /// The `(coefficient, variable)` terms, in order.
    pub fn terms(&self) -> impl Iterator<Item = (i64, Var)> + '_ {
        self.coefficients
            .iter()
            .copied()
            .zip(self.vars.iter().copied())
    }

    /// How the sum compares with the right-hand side.
    #[must_use]
    pub fn relation(&self) -> Relation {
        self.relation
    }

    /// The right-hand side.
    #[must_use]
    pub fn rhs(&self) -> i64 {
        self.rhs
    }

    /// Returns `true` if the sum can stay at most the right-hand side.
    fn caps(&self) -> bool {
        matches!(self.relation, Relation::AtMost | Relation::Equal)
    }

    /// Returns `true` if the sum can reach at least the right-hand side.
    fn floors(&self) -> bool {
        matches!(self.relation, Relation::AtLeast | Relation::Equal)
    }
}

impl<D: Domain> Propagator<D> for Linear {
    /// The smallest and largest contribution of each term.
    type Scratch = Vec<(i128, i128)>;

    fn scope(&self) -> &[Var] {
        &self.vars
    }

    fn propagate(
        &self,
        domains: &mut Domains<D>,
        bounds: &mut Vec<(i128, i128)>,
    ) -> Result<(), Inconsistent> {
        let rhs = i128::from(self.rhs);
        let (low, high) = self.term_bounds(domains, bounds);
        if (self.caps() && low > rhs) || (self.floors() && high < rhs) {
            return Err(Inconsistent);
        }

        for (term, (&var, &coefficient)) in self.vars.iter().zip(&self.coefficients).enumerate() {
            let (term_low, term_high) = bounds[term];
            let (rest_low, rest_high) = (low - term_low, high - term_high);
            let mut allowed = domains[var];
            for value in domains[var].values() {
                let contribution = i128::from(coefficient) * i128::from(value);
                let over = self.caps() && contribution + rest_low > rhs;
                let under = self.floors() && contribution + rest_high < rhs;
                if over || under {
                    allowed.remove(value);
                }
            }

            domains.restrict(var, allowed)?;
        }

        Ok(())
    }
}

impl Linear {
    /// Fills `bounds` with each term's smallest and largest contribution over the current
    /// domains, and returns their sums.
    fn term_bounds<D: Domain>(
        &self,
        domains: &Domains<D>,
        bounds: &mut Vec<(i128, i128)>,
    ) -> (i128, i128) {
        bounds.clear();
        let (mut low, mut high) = (0, 0);
        for (&var, &coefficient) in self.vars.iter().zip(&self.coefficients) {
            let contributions = domains[var]
                .values()
                .map(|value| i128::from(coefficient) * i128::from(value));
            let (term_low, term_high) =
                contributions.fold((i128::MAX, i128::MIN), |(lo, hi), c| (lo.min(c), hi.max(c)));
            bounds.push((term_low, term_high));
            low += term_low;
            high += term_high;
        }

        (low, high)
    }
}

/// Tests [`Linear`] against brute-force enumeration.
#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::{Linear, Relation};
    use crate::testing::{Rng, check_sound_against_brute_force};
    use crate::{Domains, Inconsistent, Propagator, Var};

    /// On random small instances, propagation keeps every supported value, fails only
    /// without solutions, and decides full assignments.
    #[test]
    fn sound_against_brute_force() {
        let mut rng = Rng::new(23);
        for _ in 0..3000 {
            let vars = 1 + rng.below(4);
            let coefficients: Vec<i64> = (0..vars)
                .map(|_| i64::try_from(rng.below(7)).unwrap() - 3)
                .collect();
            let relation = [Relation::AtMost, Relation::Equal, Relation::AtLeast][rng.below(3)];
            let rhs = i64::try_from(rng.below(25)).unwrap() - 8;
            let holds = |assignment: &[u32]| {
                let sum: i64 = coefficients
                    .iter()
                    .zip(assignment)
                    .map(|(&a, &x)| a * i64::from(x))
                    .sum();
                match relation {
                    Relation::AtMost => sum <= rhs,
                    Relation::Equal => sum == rhs,
                    Relation::AtLeast => sum >= rhs,
                }
            };

            check_sound_against_brute_force(&mut rng, vars, 6, holds, |scope: Vec<Var>| {
                Linear::new(coefficients.iter().copied().zip(scope), relation, rhs)
            });
        }
    }

    /// Bounds that cannot meet the right-hand side fail at once.
    #[test]
    fn infeasible_bounds_fail() {
        let mut domains = Domains::<u16>::new();
        let x = domains.add(0b11);
        let y = domains.add(0b11);

        let too_big = Linear::sum([x, y], Relation::AtLeast, 3);
        assert_eq!(
            too_big.propagate(&mut domains, &mut Vec::new()),
            Err(Inconsistent)
        );
        let negative = Linear::new([(-1, x), (2, y)], Relation::Equal, 2);
        negative.propagate(&mut domains, &mut Vec::new()).unwrap();
        assert_eq!((domains[x], domains[y]), (0b01, 0b10));
    }
}
