# constraint-search

[![crates.io](https://img.shields.io/crates/v/constraint-search.svg)](https://crates.io/crates/constraint-search)
[![docs.rs](https://img.shields.io/docsrs/constraint-search)](https://docs.rs/constraint-search)
[![CI](https://github.com/sunsided/constraint-search/actions/workflows/ci.yml/badge.svg)](https://github.com/sunsided/constraint-search/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/sunsided/constraint-search/graph/badge.svg)](https://codecov.io/gh/sunsided/constraint-search)
[![MSRV](https://img.shields.io/crates/msrv/constraint-search)](https://crates.io/crates/constraint-search)
[![License](https://img.shields.io/crates/l/constraint-search)](https://github.com/sunsided/constraint-search#license)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

Finite-domain constraint satisfaction in Rust: variables with small domains, constraints that prune them, and search for the assignments that remain.

## Scope

- Finite-domain variables with bitset domains and a trail for undoing changes on backtrack
- Propagators run to a fixpoint, including all-different with Hall-set reasoning
- Pluggable inference rules ordered by cost, for human-style solving and difficulty grading
- Backtracking search with variable and value ordering heuristics under a budget
- Solution counting with early stop, for checking that a puzzle has a unique solution
- Exact cover with dancing links

Typical uses are logic puzzles (Sudoku and its variants, nonograms, KenKen), map colouring, timetabling and small scheduling problems. Adversarial games belong in [game-tree-search](https://github.com/sunsided/game-tree-search), and path finding in [heuristic-search](https://github.com/sunsided/heuristic-search).

## Usage

```sh
cargo add constraint-search
```

A propagator is a constraint that removes values from variable domains. It reports whether
it pruned anything, so propagation can stop at a fixpoint, and reports a conflict when a
domain runs empty. Here, `x != y` over two variables whose domains are bitsets of the
values 0 to 15.

```rust
use constraint_search::Propagator;

/// `x != y`; bit `v` of a domain is set while the variable may still take the value `v`.
struct NotEqual;

impl Propagator<[u16; 2]> for NotEqual {
    fn propagate(&mut self, domains: &mut [u16; 2]) -> Option<bool> {
        let mut changed = false;
        for (fixed, other) in [(0, 1), (1, 0)] {
            let value = domains[fixed];
            // A single remaining value fixes the variable and rules that value out for the other.
            if value.is_power_of_two() && domains[other] & value != 0 {
                domains[other] &= !value;
                changed = true;
            }
        }
        if domains.contains(&0) { None } else { Some(changed) }
    }
}

// x is fixed to 3; y may be 3 or 5.
let mut domains = [1 << 3, 1 << 3 | 1 << 5];
assert_eq!(NotEqual.propagate(&mut domains), Some(true));
assert_eq!(domains, [1 << 3, 1 << 5]);
// Nothing is left to prune: propagation has reached a fixpoint.
assert_eq!(NotEqual.propagate(&mut domains), Some(false));
```

The crate is `no_std` and has no dependencies.

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
