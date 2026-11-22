//! Integration tests: whole puzzles modelled through the public API.

mod beyond;
mod cover_oracle;
mod csp_oracle;
mod props;
mod queens;
mod rules;
mod sudoku;
mod wide;

use constraint_search::Stats;

/// A budget that never stops the search.
fn unlimited(_: &Stats) -> bool {
    false
}
