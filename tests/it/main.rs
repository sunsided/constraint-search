//! Integration tests: whole puzzles modelled through the public API.

mod queens;
mod rules;
mod sudoku;
mod wide;

use constraint_search::Stats;

/// A budget that never stops the search.
fn unlimited(_: &Stats) -> bool {
    false
}
