# constraint-search

Finite-domain constraint satisfaction in Rust: variables with small domains, constraints that prune them, and search for the assignments that remain.

## Scope

- Finite-domain variables with bitset domains and a trail for undoing changes on backtrack
- Propagators run to a fixpoint, including all-different with Hall-set reasoning
- Pluggable inference rules ordered by cost, for human-style solving and difficulty grading
- Backtracking search with variable and value ordering heuristics under a budget
- Solution counting with early stop, for checking that a puzzle has a unique solution
- Exact cover with dancing links

Typical uses are logic puzzles (Sudoku and its variants, nonograms, KenKen), map colouring, timetabling and small scheduling problems. Adversarial games belong in [game-tree-search](https://github.com/sunsided/game-tree-search), and path finding in [heuristic-search](https://github.com/sunsided/heuristic-search).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/sunsided/constraint-search/blob/main/LICENSE-APACHE))
- MIT license ([LICENSE-MIT](https://github.com/sunsided/constraint-search/blob/main/LICENSE-MIT))

at your option.
