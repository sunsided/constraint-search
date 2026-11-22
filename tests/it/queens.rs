//! N-queens with a custom propagator, and map colouring: models beyond all-different alone.

use constraint_search::{
    AllDifferent, Count, Domain, Domains, ExactCover, Inconsistent, Model, Outcome, Propagator,
    Solver, ValueOrder, Var,
};

use crate::unlimited;

/// Queens in columns `a` and `b`, `distance` columns apart, are not on a shared diagonal.
struct NoDiagonal {
    /// The two queens' row variables.
    vars: [Var; 2],
    /// How many columns apart they are. Never zero.
    distance: u32,
}

impl Propagator<u16> for NoDiagonal {
    type Scratch = ();

    fn scope(&self) -> &[Var] {
        &self.vars
    }

    fn propagate(&self, domains: &mut Domains<u16>, (): &mut ()) -> Result<(), Inconsistent> {
        for (fixed, other) in [(self.vars[0], self.vars[1]), (self.vars[1], self.vars[0])] {
            if let Some(row) = domains.value(fixed) {
                domains.remove(other, row + self.distance)?;
                if let Some(below) = row.checked_sub(self.distance) {
                    domains.remove(other, below)?;
                }
            }
        }

        Ok(())
    }
}

/// N-queens: one row variable per column, distinct rows, no shared diagonals.
fn queens(n: u32) -> Model<u16> {
    let mut model = Model::new();
    let columns = model.add_vars(n as usize, u16::range(n));
    model.add(AllDifferent::new(columns.iter().copied()));

    for (i, &a) in columns.iter().enumerate() {
        for (j, &b) in columns.iter().enumerate().skip(i + 1) {
            let distance = u32::try_from(j - i).unwrap();
            model.add(NoDiagonal {
                vars: [a, b],
                distance,
            });
        }
    }

    model
}

/// The classic queens counts, matching the exact cover formulation.
#[test]
fn queens_counts() {
    for (n, expected) in [(4, 2), (6, 4), (8, 92)] {
        let model = queens(n);
        let report = Solver::new(&model).count(u64::MAX, unlimited);
        assert_eq!(report.outcome, Count::Exact(expected), "{n} queens");
    }
}

/// Least-constraining-value ordering reports the values its probes refute as failures.
#[test]
fn lcv_counts_refuted_probes() {
    let model = queens(6);
    let report = Solver::new(&model)
        .with_value_order(ValueOrder::Lcv)
        .count(u64::MAX, unlimited);

    assert_eq!(report.outcome, Count::Exact(4));
    assert!(report.stats.failures > 0);
}

/// A complete graph on four regions cannot be three-coloured.
#[test]
fn k4_is_not_three_colourable() {
    let mut model = Model::<u16>::new();
    let regions = model.add_vars(4, u16::range(3));
    for (i, &a) in regions.iter().enumerate() {
        for &b in &regions[i + 1..] {
            model.add(AllDifferent::new([a, b]));
        }
    }

    let report = Solver::new(&model).solve(unlimited);
    assert_eq!(report.outcome, Outcome::Unsatisfiable);
    assert!(report.stats.failures > 0);
}

/// One model serves solvers on several threads at once.
#[test]
fn model_is_shared_across_threads() {
    let model = queens(8);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8_u32)
            .map(|first_row| {
                let model = &model;
                scope.spawn(move || {
                    let mut solver = Solver::new(model);
                    solver.domains_mut().assign(Var::new(0), first_row).unwrap();
                    match solver.count(u64::MAX, unlimited).outcome {
                        Count::Exact(count) => count,
                        Count::AtLeast(_) => unreachable!("no limit"),
                    }
                })
            })
            .collect();

        let total: u64 = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum();
        assert_eq!(total, 92);
    });
}

/// Exact cover finds the same eight-queens count through secondary diagonal columns.
#[test]
fn exact_cover_agrees() {
    let n = 8;
    let diagonals = 2 * n - 1;
    let mut problem = ExactCover::new(2 * n, 2 * diagonals);
    for row in 0..n {
        for column in 0..n {
            let rising = 2 * n + row + column;
            let falling = 2 * n + diagonals + row + n - 1 - column;
            problem.add_row(&[row, n + column, rising, falling]);
        }
    }

    assert_eq!(problem.count(u64::MAX, unlimited).outcome, Count::Exact(92));
}
