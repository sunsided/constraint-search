# constraint-search

[![crates.io](https://img.shields.io/crates/v/constraint-search.svg)](https://crates.io/crates/constraint-search)
[![docs.rs](https://img.shields.io/docsrs/constraint-search)](https://docs.rs/constraint-search)
[![CI](https://github.com/sunsided/constraint-search/actions/workflows/ci.yml/badge.svg)](https://github.com/sunsided/constraint-search/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/sunsided/constraint-search/graph/badge.svg)](https://codecov.io/gh/sunsided/constraint-search)
[![MSRV](https://img.shields.io/crates/msrv/constraint-search)](https://crates.io/crates/constraint-search)
[![License](https://img.shields.io/crates/l/constraint-search)](https://github.com/sunsided/constraint-search#license)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

Finite-domain constraint satisfaction in Rust: variables with small domains, constraints that prune them, and search for the assignments that remain.

## Who this is for

- Puzzle generators and solvers (Sudoku and its variants, nonograms, Minesweeper, Kakuro,
  logic grids): uniqueness checks stop at the second solution, and the human-style rules
  grade difficulty by the costliest technique a solve needs and give the next hint step.
- Tutoring, hint and explainable configuration tools, and small scheduling or allocation
  models: rules carry names, and `Trace::why_removed` says which deduction ruled a value
  out, so "why can't I choose this?" has an answer next to the solution.
- Game AI with hidden information, such as card locations or unrevealed Minesweeper cells:
  the backbone gives the certain facts, marginals the probability of each value, and
  sampling draws consistent hidden states to plan with, without enumerating worlds by hand.
- Embedded and `no_std` users: the core needs only `alloc`, and parallel search is an
  opt-in `std` feature.
- For large industrial models, optimisation objectives or huge domains, use
  [OR-Tools CP-SAT](https://developers.google.com/optimization), [Gecode](https://www.gecode.org/)
  or a MIP solver. For search over the moves of an adversarial game, use
  [game-tree-search](https://github.com/sunsided/game-tree-search); for shortest paths and
  state-space search with costs, [heuristic-search](https://github.com/sunsided/heuristic-search);
  for planning when the outcomes of actions are uncertain, rather than facts hidden,
  [stochastic-search](https://github.com/sunsided/stochastic-search).

## Scope

- Finite-domain variables with bitset domains and a trail for undoing changes on backtrack
- Propagators run to a fixpoint, including all-different with complete Hall-set reasoning,
  constraints given by their allowed tuples, weighted sums and nonogram lines
- Pluggable inference rules ordered by cost, for human-style solving, hints, explanations and
  difficulty grading
- Backtracking search with variable and value ordering heuristics under a budget, resumable
  so the work can be spread across frames or ticks
- Solution counting with early stop, for checking that a puzzle has a unique solution
- Problems with many solutions: the values certain in every solution (backbone), each
  value's probability (marginals), and random solutions to sample hidden information
- Exact cover with dancing links

Typical uses are logic puzzles (Sudoku and its variants, nonograms, KenKen, Kakuro, logic grids, cryptarithms), Minesweeper and card-location deduction, map colouring, timetabling and small scheduling problems.

## Usage

```sh
cargo add constraint-search
```

A [`Model`] holds the variables and constraints and can be shared between threads. A
[`Solver`] borrows it and reuses its working state from one search to the next. Here, the
3×3 Latin squares: every row and column holds the values 0, 1 and 2 once.

```rust
use constraint_search::{AllDifferent, Count, Domain, Model, Outcome, Solver, Stats};

let mut model = Model::<u16>::new();
let cells = model.add_vars(9, u16::range(3));
for i in 0..3 {
    model.add(AllDifferent::new((0..3).map(|j| cells[3 * i + j])));
    model.add(AllDifferent::new((0..3).map(|j| cells[3 * j + i])));
}

// No budget limit: the closure never asks the search to stop.
let unlimited = |_: &Stats| false;
let mut solver = Solver::new(&model);
assert_eq!(solver.count(u64::MAX, unlimited).outcome, Count::Exact(12));

// One clue leaves several squares; a second one makes the square unique.
solver.domains_mut().assign(cells[0], 0)?;
assert_eq!(solver.count(2, unlimited).outcome, Count::AtLeast(2));

solver.domains_mut().assign(cells[4], 1)?;
assert_eq!(solver.count(2, unlimited).outcome, Count::Exact(1));
let report = solver.solve(unlimited);
assert_eq!(report.outcome, Outcome::Solved(vec![0, 2, 1, 2, 1, 0, 1, 0, 2]));
# Ok::<(), constraint_search::Inconsistent>(())
```

Custom constraints implement [`Propagator`]; [`Table`] lists allowed tuples for small ones,
and [`Linear`] and [`Line`] cover sums and nonogram clues.
For human-style solving, [`Rules`] applies costed techniques ([`VariableSubset`],
[`ValueSubset`], [`GroupIntersection`] or your own [`Rule`]), cheapest first. Each step can serve
as a hint, and the most expensive rule a solve needed grades the puzzle.

[`Model`]: https://docs.rs/constraint-search/latest/constraint_search/struct.Model.html
[`Solver`]: https://docs.rs/constraint-search/latest/constraint_search/struct.Solver.html
[`Propagator`]: https://docs.rs/constraint-search/latest/constraint_search/trait.Propagator.html
[`Table`]: https://docs.rs/constraint-search/latest/constraint_search/struct.Table.html
[`Linear`]: https://docs.rs/constraint-search/latest/constraint_search/struct.Linear.html
[`Line`]: https://docs.rs/constraint-search/latest/constraint_search/struct.Line.html
[`Rules`]: https://docs.rs/constraint-search/latest/constraint_search/struct.Rules.html
[`Rule`]: https://docs.rs/constraint-search/latest/constraint_search/trait.Rule.html
[`VariableSubset`]: https://docs.rs/constraint-search/latest/constraint_search/struct.VariableSubset.html
[`ValueSubset`]: https://docs.rs/constraint-search/latest/constraint_search/struct.ValueSubset.html
[`GroupIntersection`]: https://docs.rs/constraint-search/latest/constraint_search/struct.GroupIntersection.html

The crate is `no_std` and needs `alloc`. The `parallel` feature (which needs `std`) adds
`ParallelSolver`: the search tree split across threads, or a portfolio of branching
heuristics racing each other, stopping all threads at the first solution or a count limit. Its one dependency is
[search-core](https://github.com/sunsided/search-core), for the `Budget` trait.

## Minimum supported Rust version

Rust 1.85 (edition 2024). The MSRV applies to the library; tests may need a newer
toolchain. It may be raised in a minor release when a dependency or a language feature
calls for it.

## Contributing

See [CONTRIBUTING.md](https://github.com/sunsided/constraint-search/blob/main/CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/sunsided/constraint-search/blob/main/LICENSE-APACHE))
- MIT license ([LICENSE-MIT](https://github.com/sunsided/constraint-search/blob/main/LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
