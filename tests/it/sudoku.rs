//! Sudoku as all-different groups: solving, uniqueness and solver reuse.

use constraint_search::{
    AllDifferent, Count, Domain, GroupIntersection, Model, Outcome, Rules, Solver, Stats,
    ValueOrder, ValueSubset, Var, VarOrder, VariableSubset,
};

use crate::unlimited;

/// A Sudoku of `box_size² × box_size²` cells: its model and its row, column and box groups.
pub struct Sudoku {
    /// One variable per cell in row-major order, values `0..side`.
    pub model: Model<u32>,
    /// The rows, then the columns, then the boxes.
    pub groups: Vec<Vec<Var>>,
}

impl Sudoku {
    /// An empty grid with boxes of `box_size × box_size` cells.
    pub fn new(box_size: usize) -> Self {
        let side = box_size * box_size;
        let mut model = Model::new();
        let cells = model.add_vars(side * side, u32::range(u32::try_from(side).unwrap()));

        let rows = (0..side).map(|r| (0..side).map(|c| cells[r * side + c]).collect());
        let columns = (0..side).map(|c| (0..side).map(|r| cells[r * side + c]).collect());
        let boxes = (0..side).map(|b| {
            let (top, left) = (b / box_size * box_size, b % box_size * box_size);
            (0..side)
                .map(|i| cells[(top + i / box_size) * side + left + i % box_size])
                .collect()
        });
        let groups: Vec<Vec<Var>> = rows.chain(columns).chain(boxes).collect();

        for group in &groups {
            model.add(AllDifferent::new(group.iter().copied()));
        }

        Self { model, groups }
    }

    /// A solver with the clues of `puzzle` applied: digits `1..=9` are clues, `.` is empty.
    pub fn solver_for(&self, puzzle: &str) -> Solver<'_, u32> {
        let mut solver = Solver::new(&self.model);
        apply_clues(&mut solver, puzzle);

        solver
    }

    /// The usual human rule set over the groups, with costs from easy to hard.
    pub fn rules(&self) -> Rules<u32> {
        let mut rules = Rules::new();
        rules
            .add(VariableSubset::new(1, 1, self.groups.clone()))
            .add(ValueSubset::new(1, 2, self.groups.clone()))
            .add(GroupIntersection::new(3, self.groups.clone()))
            .add(VariableSubset::new(2, 4, self.groups.clone()))
            .add(ValueSubset::new(2, 5, self.groups.clone()))
            .add(VariableSubset::new(3, 6, self.groups.clone()))
            .add(ValueSubset::new(3, 7, self.groups.clone()));

        rules
    }
}

/// Fixes each clue cell of `puzzle` in the solver's domains.
fn apply_clues(solver: &mut Solver<'_, u32>, puzzle: &str) {
    let cells = puzzle.chars().filter(|c| !c.is_whitespace());
    for (index, cell) in cells.enumerate() {
        if let Some(digit) = cell.to_digit(10) {
            let var = Var::new(u32::try_from(index).unwrap());
            solver.domains_mut().assign(var, digit - 1).unwrap();
        }
    }
}

/// Formats a solution as digits `1..=9` in row-major order.
fn digits(values: &[u32]) -> String {
    values
        .iter()
        .map(|value| char::from_digit(value + 1, 10).unwrap())
        .collect()
}

/// A puzzle with a unique solution that needs only singles.
const EASY: &str = "
    53..7.... 6..195... .98....6. 8...6...3 4..8.3..1 7...2...6 .6....28. ...419..5 ....8..79";

/// The solution of [`EASY`].
const EASY_SOLUTION: &str =
    "534678912672195348198342567859761423426853791713924856961537284287419635345286179";

/// A hard puzzle (Arto Inkala's "AI Escargot") that needs search.
const ESCARGOT: &str = "
    1....7.9. .3..2...8 ..96..5.. ..53..9.. .1..8...2 6....4... 3......1. .4......7 ..7...3..";

/// The easy puzzle solves to its known solution, and the count proves it unique.
#[test]
fn solves_and_proves_unique() {
    let sudoku = Sudoku::new(3);
    let mut solver = sudoku.solver_for(EASY);

    let report = solver.solve(unlimited);
    let Outcome::Solved(values) = report.outcome else {
        panic!("solvable")
    };
    assert_eq!(digits(&values), EASY_SOLUTION);
    assert_eq!(solver.count(2, unlimited).outcome, Count::Exact(1));
}

/// Every branching heuristic finds the same unique solution of a hard puzzle.
#[test]
fn heuristics_agree_on_hard_puzzle() {
    let sudoku = Sudoku::new(3);
    let orders = [
        (VarOrder::Mrv, ValueOrder::Ascending),
        (VarOrder::Mrv, ValueOrder::Lcv),
        (VarOrder::Input, ValueOrder::Descending),
    ];

    let solutions = orders.map(|(var_order, value_order)| {
        let mut solver = sudoku
            .solver_for(ESCARGOT)
            .with_var_order(var_order)
            .with_value_order(value_order);
        assert_eq!(solver.count(2, unlimited).outcome, Count::Exact(1));
        solver.solve(unlimited).outcome
    });

    assert!(matches!(solutions[0], Outcome::Solved(_)));
    assert!(
        solutions[1..]
            .iter()
            .all(|outcome| *outcome == solutions[0])
    );
}

/// A uniqueness check hinted with the known solution searches less than an unhinted one.
#[test]
fn hint_speeds_up_uniqueness_checks() {
    let sudoku = Sudoku::new(3);
    let mut solver = sudoku.solver_for(ESCARGOT);
    let plain = solver.count(2, unlimited);
    let Outcome::Solved(solution) = solver.solve(unlimited).outcome else {
        panic!("solvable")
    };

    solver.set_hint(&solution);
    let hinted = solver.count(2, unlimited);
    assert_eq!(hinted.outcome, Count::Exact(1));
    assert_eq!(plain.outcome, hinted.outcome);
    assert!(hinted.stats.failures <= plain.stats.failures);
}

/// Reset returns to the empty grid, so one solver serves many puzzles.
#[test]
fn reset_reuses_the_solver() {
    let sudoku = Sudoku::new(3);
    let mut solver = sudoku.solver_for(EASY);
    assert!(matches!(
        solver.solve(unlimited).outcome,
        Outcome::Solved(_)
    ));

    solver.reset();
    assert_eq!(solver.domains()[Var::new(0)].len(), 9);

    apply_clues(&mut solver, ESCARGOT);
    assert_eq!(solver.count(2, unlimited).outcome, Count::Exact(1));
}

/// A tight budget stops the search with what it has seen so far.
#[test]
fn budget_stops_the_search() {
    let sudoku = Sudoku::new(3);
    let mut solver = sudoku.solver_for("");

    let report = solver.count(u64::MAX, |stats: &Stats| stats.nodes >= 500);
    assert!(matches!(report.outcome, Count::AtLeast(_)));
    assert_eq!(report.stats.nodes, 500);
}

/// Searches leave the domains as they found them.
#[test]
fn search_restores_domains() {
    let sudoku = Sudoku::new(3);
    let mut solver = sudoku.solver_for(EASY);
    let before = solver.domains().clone();

    let _ = solver.count(u64::MAX, unlimited);
    let after = solver.domains();
    assert!(before.vars().all(|var| before[var] == after[var]));
}

/// Two equal clues in a row are refuted at the root.
#[test]
fn contradictory_clues_are_unsatisfiable() {
    let sudoku = Sudoku::new(3);
    let mut solver = sudoku.solver_for("11");

    let report = solver.solve(unlimited);
    assert_eq!(report.outcome, Outcome::Unsatisfiable);
    assert_eq!(report.stats.nodes, 1);
}

/// The easy puzzle falls to singles alone, and the rules agree with search.
#[test]
fn rules_solve_and_grade_easy_puzzle() {
    let sudoku = Sudoku::new(3);
    let rules = sudoku.rules();
    let mut solver = sudoku.solver_for(EASY);

    let trace = rules.solve(solver.domains_mut(), false).unwrap();
    assert!(trace.solved);
    assert!(trace.grade().unwrap() <= 2, "singles suffice");
    assert_eq!(digits(&solver.domains().solution().unwrap()), EASY_SOLUTION);
}

/// The hard puzzle stalls the rules, so it gets no grade.
#[test]
fn rules_stall_on_hard_puzzle() {
    let sudoku = Sudoku::new(3);
    let mut solver = sudoku.solver_for(ESCARGOT);

    let trace = sudoku.rules().solve(solver.domains_mut(), false).unwrap();
    assert!(!trace.solved);
    assert_eq!(trace.grade(), None);
}

/// A parallel uniqueness check agrees with the sequential one on a hard puzzle.
#[cfg(feature = "parallel")]
#[test]
fn parallel_uniqueness_check() {
    use constraint_search::ParallelSolver;

    let sudoku = Sudoku::new(3);
    let clues = sudoku.solver_for(ESCARGOT).domains().clone();
    let mut model = Model::new();
    for var in clues.vars() {
        model.add_var(clues[var]);
    }

    for group in &sudoku.groups {
        model.add(AllDifferent::new(group.iter().copied()));
    }

    let report = ParallelSolver::new(&model, 4).count(2, unlimited);
    assert_eq!(report.outcome, Count::Exact(1));
}
