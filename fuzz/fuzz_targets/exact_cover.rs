//! Fuzzes dancing links against trying every subset of rows, on small exact cover problems
//! decoded from the input bytes.

#![no_main]

#[path = "../../tests/it/cover_oracle.rs"]
mod cover_oracle;

use arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

use cover_oracle::Cover;

fuzz_target!(|data: &[u8]| {
    if let Ok(problem) = decode(&mut Unstructured::new(data)) {
        problem.check();
    }
});

/// Up to five primary and three secondary columns, and up to ten rows.
fn decode(input: &mut Unstructured<'_>) -> Result<Cover> {
    let primary = input.int_in_range(0..=5)?;
    let secondary = input.int_in_range(0..=3)?;
    let rows = (0..input.int_in_range(0..=10)?)
        .map(|_| row(input, primary + secondary))
        .collect::<Result<_>>()?;

    Ok(Cover {
        primary,
        secondary,
        rows,
    })
}

/// A row: any distinct columns of `0..columns`, ascending.
fn row(input: &mut Unstructured<'_>, columns: usize) -> Result<Vec<usize>> {
    let mut row = Vec::new();
    for column in 0..columns {
        if input.arbitrary()? {
            row.push(column);
        }
    }

    Ok(row)
}
