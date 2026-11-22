//! Domains wider than a machine word, through [`BitSet`].

use constraint_search::{AllDifferent, BitSet, Count, Domain, Model, Outcome, Solver, Table};

use crate::unlimited;

/// A domain of the given values over two 64-bit words.
fn domain(values: &[u32]) -> BitSet<2> {
    let mut domain = BitSet::empty();
    for &value in values {
        domain.insert(value);
    }

    domain
}

/// Values on both sides of the word boundary propagate, count and solve like small ones.
#[test]
fn values_beyond_one_word() {
    let mut model = Model::new();
    let x = model.add_var(domain(&[3, 64]));
    let y = model.add_var(domain(&[64, 65, 127]));
    let z = model.add_var(domain(&[64, 65, 127]));
    model.add(AllDifferent::new([x, y, z]));
    model.add(Table::new([x, y], [[3, 127], [64, 65], [64, 127]]));

    let mut solver = Solver::new(&model);
    assert_eq!(solver.count(u64::MAX, unlimited).outcome, Count::Exact(4));

    solver.domains_mut().assign(x, 3).unwrap();
    let report = solver.solve(unlimited);
    assert_eq!(report.outcome, Outcome::Solved(vec![3, 127, 64]));
}
