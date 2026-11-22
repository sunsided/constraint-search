//! Fuzzes propagation, search, counting and the rules against brute-force enumeration on
//! small problems decoded from the input bytes.

#![no_main]

#[path = "../../tests/it/csp_oracle.rs"]
mod csp_oracle;

use arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

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

/// An all-different over any variables, or a table over up to three with up to eight tuples.
fn constraint(input: &mut Unstructured<'_>, vars: usize) -> Result<Constraint> {
    if input.arbitrary()? {
        return Ok(Constraint::AllDifferent(scope(input, vars, vars)?));
    }

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
