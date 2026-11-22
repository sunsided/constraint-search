//! [`Table`]: a constraint given by its allowed tuples.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Propagator, Var};

/// The variables take one of the listed tuples of values.
///
/// Any relation over a few variables can be written this way: a KenKen cage (every tuple
/// with the right sum or product), the allowed colour pairs of two regions, the shifts a
/// worker can take on consecutive days. Propagation is complete: afterwards every value left
/// is part of an allowed tuple whose other values are all still possible. Each run scans the
/// tuples once.
///
/// # Examples
///
/// ```
/// use constraint_search::{Domain, Domains, Propagator, Table};
///
/// let mut domains = Domains::<u16>::new();
/// let x = domains.add(u16::range(4));
/// let y = domains.add(u16::range(4));
///
/// // A two-cell cage whose values sum to 5.
/// let cage = Table::new([x, y], [[2, 3], [3, 2]]);
/// cage.propagate(&mut domains)?;
/// assert!(domains[x].values().eq([2, 3]));
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The constrained variables. Never holds a variable twice.
    vars: Vec<Var>,
    /// The allowed tuples back to back, each `vars.len()` values long.
    tuples: Vec<u32>,
}

impl Table {
    /// The constraint that `vars` take one of `tuples`, each listing one value per variable.
    ///
    /// # Panics
    ///
    /// Panics if `vars` is empty, holds a variable twice, or if a tuple's length differs from
    /// the number of variables.
    #[must_use]
    pub fn new<T: AsRef<[u32]>>(
        vars: impl IntoIterator<Item = Var>,
        tuples: impl IntoIterator<Item = T>,
    ) -> Self {
        let vars: Vec<Var> = vars.into_iter().collect();
        assert!(!vars.is_empty(), "a table needs at least one variable");

        let mut sorted = vars.clone();
        sorted.sort_unstable();
        assert!(
            sorted.windows(2).all(|pair| pair[0] != pair[1]),
            "a table lists a variable twice"
        );

        let mut flat = Vec::new();
        for tuple in tuples {
            let tuple = tuple.as_ref();
            assert_eq!(
                tuple.len(),
                vars.len(),
                "a tuple needs one value per variable"
            );
            flat.extend_from_slice(tuple);
        }

        Self { vars, tuples: flat }
    }

    /// The allowed tuples.
    pub fn tuples(&self) -> impl Iterator<Item = &[u32]> {
        self.tuples.chunks_exact(self.vars.len())
    }
}

impl<D: Domain> Propagator<D> for Table {
    fn scope(&self) -> &[Var] {
        &self.vars
    }

    fn propagate(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent> {
        let mut supported = vec![D::empty(); self.vars.len()];
        let possible = self.tuples().filter(|tuple| {
            self.vars
                .iter()
                .zip(*tuple)
                .all(|(&var, &value)| domains[var].contains(value))
        });
        for tuple in possible {
            for (support, &value) in supported.iter_mut().zip(tuple) {
                support.insert(value);
            }
        }

        for (&var, support) in self.vars.iter().zip(supported) {
            domains.restrict(var, support)?;
        }

        Ok(())
    }
}

/// Tests [`Table`] against brute-force enumeration.
#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::Table;
    use crate::testing::{Rng, check_against_brute_force};

    /// On random small instances with random tuples, propagation keeps exactly the values
    /// with a support.
    #[test]
    fn matches_brute_force() {
        let mut rng = Rng::new(11);
        for _ in 0..2000 {
            let vars = 1 + rng.below(4);
            let tuples: Vec<Vec<u32>> = (0..rng.below(12))
                .map(|_| {
                    (0..vars)
                        .map(|_| u32::try_from(rng.below(5)).unwrap())
                        .collect()
                })
                .collect();
            let allowed = |assignment: &[u32]| tuples.iter().any(|tuple| tuple == assignment);

            check_against_brute_force(&mut rng, vars, 5, allowed, |scope| {
                Table::new(scope, &tuples)
            });
        }
    }
}
