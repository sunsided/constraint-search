# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/sunsided/constraint-search/releases/tag/v0.1.0) - 2026-11-22

### Added

- Add dormant release automation with release-plz
- Add CONTRIBUTING.md
- Add git hooks and Copilot review instructions
- Add README badges, a usage example and package excludes
- Add REUSE licensing metadata
- Add supply-chain checks
- Add Codecov configuration
- Add Taskfile and CI workflows

### Changed

- Share search result and domain helpers across solvers
- Lead the crate description with what it is for
- Say who the crate is for in the README
- Abandon a paused search before analysing, and make the limits exact
- Backbone, marginals and solution sampling for undetermined problems
- Sum and line constraints, rules for any propagator, and explanations
- Name the all-different rules generically, with Sudoku terms as aliases
- Parallel search behind a parallel feature
- Keep every search change above the level where the search began
- Try a hinted value first when branching
- Budget and stats for ExactCover
- Resumable search through an explicit choice-point stack
- Reuse scratch memory so warmed-up solves allocate nothing
- Count values refuted by least-constraining-value probes as failures
- Let hidden subsets include values only one variable can take
- Charge the budget per examined node, and search nothing for a zero limit
- Test the budget, limit and accessor paths coverage missed
- Build fuzz targets for the gnu host in CI
- Fuzz propagation and exact cover against the brute-force oracles
- Check propagation, search, rules and exact cover against brute force with proptest
- Document the crate and test it on whole puzzles
- Exact cover with dancing links
- Backtracking search with solution counting under a budget
- Cost-ordered inference rules with hints and grading
- All-different by Regin matching, and table constraints
- Propagation to a fixpoint over a shared model
- Variable store with a trail for backtracking
- Bitset domains from u16 to u128 and a wider BitSet
- Separate the propagator examples into paragraphs
- Test the Propagator trait through a minimal not-equal constraint
- Tighten lints and forbid unsafe code
- Render mermaid diagrams on docs.rs and doctest the README
- Reserve the crate name with a 0.0.0 placeholder

### Dependencies

- Bump taiki-e/install-action to v2.90.8

### Fixed

- Fix samples that a hint pinned to one solution
