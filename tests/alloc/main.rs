//! Allocation check: once warmed up, a solver searches without touching the heap.
//!
//! This is its own test binary because it replaces the global allocator with a counting one,
//! and it holds a single test so no other test allocates while a region is measured.

use std::alloc::System;
use std::ops::ControlFlow;

use constraint_search::{
    AllDifferent, Count, Domain, Model, Outcome, Progress, Solver, Stats, Table, ValueOrder, Var,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

/// Counts every allocation, so the test can measure a region.
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// A budget that never stops the search.
fn unlimited(_: &Stats) -> bool {
    false
}

/// A 9×9 Sudoku: rows, columns and boxes all different.
fn sudoku() -> Model<u16> {
    let mut model = Model::new();
    let cells = model.add_vars(81, u16::range(9));
    for i in 0..9 {
        model.add(AllDifferent::new((0..9).map(|j| cells[9 * i + j])));
        model.add(AllDifferent::new((0..9).map(|j| cells[9 * j + i])));

        let (top, left) = (i / 3 * 3, i % 3 * 3);
        model.add(AllDifferent::new(
            (0..9).map(|j| cells[(top + j / 3) * 9 + left + j % 3]),
        ));
    }

    model
}

/// Resets the solver and fixes the clues of `puzzle`: digits `1..=9`, `.` for empty.
fn load(solver: &mut Solver<'_, u16>, puzzle: &str) {
    solver.reset();
    let cells = puzzle.chars().filter(|c| !c.is_whitespace());
    for (index, cell) in cells.enumerate() {
        if let Some(digit) = cell.to_digit(10) {
            let var = Var::new(u32::try_from(index).unwrap());
            solver.domains_mut().assign(var, digit - 1).unwrap();
        }
    }
}

/// A hard puzzle with a unique solution (Arto Inkala's "AI Escargot").
const ESCARGOT: &str = "
    1....7.9. .3..2...8 ..96..5.. ..53..9.. .1..8...2 6....4... 3......1. .4......7 ..7...3..";

/// Runs `work` once to warm up, then again under measurement; returns the allocations of the
/// second run.
fn allocations_after_warm_up(mut work: impl FnMut()) -> usize {
    work();

    let region = Region::new(GLOBAL);
    work();
    region.change().allocations + region.change().reallocations
}

/// Uniqueness checks, enumeration, propagation with tables, and least-constraining-value
/// ordering all run allocation-free once their buffers have grown.
#[test]
fn warmed_up_solves_do_not_allocate() {
    let model = sudoku();
    let mut solver = Solver::new(&model);
    let unique = allocations_after_warm_up(|| {
        load(&mut solver, ESCARGOT);
        assert_eq!(solver.count(2, unlimited).outcome, Count::Exact(1));
    });
    assert_eq!(unique, 0, "uniqueness check");

    let mut lcv = Solver::new(&model).with_value_order(ValueOrder::Lcv);
    let ordered = allocations_after_warm_up(|| {
        load(&mut lcv, ESCARGOT);
        let _ = lcv.for_each_solution(unlimited, |_| ControlFlow::Continue(()));
    });
    assert_eq!(ordered, 0, "least-constraining-value enumeration");

    let mut sliced = Solver::new(&model);
    let resumed = allocations_after_warm_up(|| {
        load(&mut sliced, ESCARGOT);
        sliced.begin();
        while sliced.resume(|stats: &Stats| stats.nodes >= 5).outcome != Progress::Complete {}
    });
    assert_eq!(resumed, 0, "resumable search in slices");

    let mut cages = Model::<u16>::new();
    let vars = cages.add_vars(4, u16::range(4));
    cages.add(AllDifferent::new(vars.iter().copied()));
    cages.add(Table::new(
        [vars[0], vars[1]],
        [[0, 3], [3, 0], [1, 2], [2, 1]],
    ));
    let mut solver = Solver::new(&cages);
    let tables = allocations_after_warm_up(|| {
        solver.reset();
        assert_eq!(solver.count(u64::MAX, unlimited).outcome, Count::Exact(8));
    });
    assert_eq!(tables, 0, "table propagation");

    let solving = allocations_after_warm_up(|| {
        solver.reset();
        assert!(matches!(
            solver.solve(unlimited).outcome,
            Outcome::Solved(_)
        ));
    });
    assert_eq!(solving, 1, "solve allocates only the solution it returns");
}
