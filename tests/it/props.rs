//! Property tests: random small problems checked against the brute-force oracles.
//!
//! `PROPTEST_CASES` sets the number of cases (`task test:prop` runs 10000). Failing cases
//! shrink to a minimal problem and are kept in `proptest-regressions/`.

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::subsequence;

use crate::cover_oracle::Cover;
use constraint_search::Relation;

use crate::csp_oracle::{Constraint, Csp, VALUES};

/// A constraint over some of the variables `0..vars`: all-different over any subset, a table
/// over up to three variables with up to eight tuples, a linear constraint over up to three,
/// or a nonogram line over any subset with up to two runs.
fn constraint(vars: usize) -> impl Strategy<Value = Constraint> {
    let all_different =
        subsequence((0..vars).collect::<Vec<_>>(), 1..=vars).prop_map(Constraint::AllDifferent);
    let table =
        subsequence((0..vars).collect::<Vec<_>>(), 1..=vars.min(3)).prop_flat_map(|scope| {
            let arity = scope.len();
            vec(vec(0..VALUES, arity), 0..8)
                .prop_map(move |tuples| Constraint::Table(scope.clone(), tuples))
        });

    let linear = (
        subsequence((0..vars).collect::<Vec<_>>(), 1..=vars.min(3)),
        vec(-3..=3_i64, 3),
        0..3_usize,
        -8..=16_i64,
    )
        .prop_map(|(scope, coefficients, relation, rhs)| {
            let terms = coefficients.into_iter().zip(scope).collect();
            let relation = [Relation::AtMost, Relation::Equal, Relation::AtLeast][relation];
            Constraint::Linear(terms, relation, rhs)
        });
    let line = (
        subsequence((0..vars).collect::<Vec<_>>(), 1..=vars),
        vec(1..=3_usize, 0..=2),
    )
        .prop_map(|(cells, runs)| Constraint::Line(cells, runs));

    prop_oneof![all_different, table, linear, line]
}

/// A problem of one to five variables over `0..VALUES` with up to four constraints.
fn csp() -> impl Strategy<Value = Csp> {
    (1..=5_usize).prop_flat_map(|vars| {
        let full = (1_u16 << VALUES) - 1;
        (vec(1..=full, vars), vec(constraint(vars), 0..=4)).prop_map(|(domains, constraints)| Csp {
            domains,
            constraints,
        })
    })
}

/// An exact cover problem of up to five primary and three secondary columns and ten rows.
fn cover() -> impl Strategy<Value = Cover> {
    (0..=5_usize, 0..=3_usize).prop_flat_map(|(primary, secondary)| {
        let columns: Vec<usize> = (0..primary + secondary).collect();
        let row = subsequence(columns.clone(), 0..=columns.len());
        vec(row, 0..=10).prop_map(move |rows| Cover {
            primary,
            secondary,
            rows,
        })
    })
}

proptest! {
    /// Propagation, search, counting and rules all agree with enumeration.
    #[test]
    fn csp_matches_brute_force(problem in csp()) {
        problem.check();
    }

    /// Dancing links count and solve like trying every subset of rows.
    #[test]
    fn exact_cover_matches_brute_force(problem in cover()) {
        problem.check();
    }
}
