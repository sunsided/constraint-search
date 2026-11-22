//! Fuzzes propagation, search, counting and the rules against brute-force enumeration on
//! small problems decoded from the input bytes.

#![no_main]

#[path = "../../tests/it/csp_oracle.rs"]
mod csp_oracle;

use arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

use constraint_search::Relation;
use csp_oracle::{Constraint, Csp, VALUES};

fuzz_target!(|data: &[u8]| {
    if let Ok(problem) = decode(&mut Unstructured::new(data)) {
        problem.check();
    }
});

/// A problem of one to five variables over `0..VALUES` with up to four constraints.
fn decode(input: &mut Unstructured<'_>) -> Result<Csp> {
    let vars = input.int_in_range(1..=5)?;
    let full = (1_u16 << VALUES) - 1;
    let domains = (0..vars)
        .map(|_| input.int_in_range(1..=full))
        .collect::<Result<_>>()?;
    let constraints = (0..input.int_in_range(0..=4)?)
        .map(|_| constraint(input, vars))
        .collect::<Result<_>>()?;

    Ok(Csp {
        domains,
        constraints,
    })
}

/// An all-different over any variables, a table over up to three with up to eight tuples, a
/// linear constraint over up to three, or a nonogram line with up to two runs.
fn constraint(input: &mut Unstructured<'_>, vars: usize) -> Result<Constraint> {
    match input.int_in_range(0..=3)? {
        0 => Ok(Constraint::AllDifferent(scope(input, vars, vars)?)),
        1 => table(input, vars),
        2 => linear(input, vars),
        _ => {
            let cells = scope(input, vars, vars)?;
            let runs = (0..input.int_in_range(0..=2)?)
                .map(|_| input.int_in_range(1..=3))
                .collect::<Result<_>>()?;
            Ok(Constraint::Line(cells, runs))
        }
    }
}

/// A table over up to three variables with up to eight tuples.
fn table(input: &mut Unstructured<'_>, vars: usize) -> Result<Constraint> {
    let scope = scope(input, vars, 3)?;
    let tuples = (0..input.int_in_range(0..=8)?)
        .map(|_| {
            (0..scope.len())
                .map(|_| input.int_in_range(0..=VALUES - 1))
                .collect()
        })
        .collect::<Result<_>>()?;

    Ok(Constraint::Table(scope, tuples))
}

/// A linear constraint over up to three variables, with small coefficients.
fn linear(input: &mut Unstructured<'_>, vars: usize) -> Result<Constraint> {
    let terms = scope(input, vars, 3)?
        .into_iter()
        .map(|var| Ok((input.int_in_range(-3..=3)?, var)))
        .collect::<Result<_>>()?;
    let relation = *input.choose(&[Relation::AtMost, Relation::Equal, Relation::AtLeast])?;

    Ok(Constraint::Linear(
        terms,
        relation,
        input.int_in_range(-8..=16)?,
    ))
}

/// Between one and `max` distinct variables of `0..vars`, ascending.
fn scope(input: &mut Unstructured<'_>, vars: usize, max: usize) -> Result<Vec<usize>> {
    let mut scope = Vec::new();
    for var in 0..vars {
        if input.arbitrary()? {
            scope.push(var);
        }
    }

    if scope.is_empty() {
        scope.push(input.choose_index(vars)?);
    }

    scope.truncate(max);
    Ok(scope)
}
