//! Rule soundness: no rule removes a value that a solution uses, on random 4×4 Sudoku
//! puzzles and on random overlapping groups whose values need not all be used.

use std::ops::ControlFlow;

use constraint_search::{
    AllDifferent, Domains, HiddenSubset, Intersection, Model, NakedSubset, Rules, Solver, Var,
};

use crate::sudoku::Sudoku;
use crate::unlimited;

/// A xorshift64 step, for reproducible random clues.
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Every solution of the solver's current domains, by exhaustive search.
fn all_solutions(solver: &mut constraint_search::Solver<'_, u32>) -> Vec<Vec<u32>> {
    let mut solutions = Vec::new();
    solver.for_each_solution(unlimited, |domains| {
        solutions.push(domains.solution().unwrap());
        ControlFlow::Continue(())
    });

    solutions
}

/// Returns `true` if every value of `solution` is still in `domains`.
fn keeps(domains: &Domains<u32>, solution: &[u32]) -> bool {
    solution
        .iter()
        .zip(domains.vars())
        .all(|(&value, var)| domains[var] & (1 << value) != 0)
}

/// Random clues on the 4×4 grid, then the full rule set; every solution must survive.
#[test]
fn rules_never_remove_a_solution() {
    let sudoku = Sudoku::new(2);
    let rules = sudoku.rules();
    let mut state = 0x9e37_79b9_7f4a_7c15;

    for _ in 0..300 {
        let mut solver = sudoku.solver_for("");
        for _ in 0..(next(&mut state) % 7) {
            let var = Var::new(u32::try_from(next(&mut state) % 16).unwrap());
            let value = u32::try_from(next(&mut state) % 4).unwrap();
            let _ = solver.domains_mut().assign(var, value);
        }

        let solutions = all_solutions(&mut solver);
        let result = rules.solve(solver.domains_mut(), false);

        match result {
            Ok(_) => assert!(
                solutions
                    .iter()
                    .all(|solution| keeps(solver.domains(), solution))
            ),
            Err(_) => assert!(solutions.is_empty(), "rules refuted a solvable puzzle"),
        }
    }
}

/// Random domains over `0..6` for six variables, never empty.
fn random_domains(state: &mut u64) -> Vec<u16> {
    (0..6)
        .map(|_| {
            loop {
                let domain = u16::try_from(next(state) & 0b11_1111).unwrap();
                if domain != 0 {
                    break domain;
                }
            }
        })
        .collect()
}

/// Two random overlapping groups of the variables `0..6`, each of at least two variables.
fn random_groups(state: &mut u64) -> [Vec<Var>; 2] {
    [0, 1].map(|_| {
        loop {
            let mask = next(state) & 0b11_1111;
            if mask.count_ones() >= 2 {
                break (0..6_u32)
                    .filter(|bit| mask & (1 << bit) != 0)
                    .map(Var::new)
                    .collect::<Vec<_>>();
            }
        }
    })
}

/// On groups whose values need not all be used, every rule keeps every solution.
#[test]
fn rules_are_sound_beyond_permutations() {
    let mut state = 0x2545_f491_4f6c_dd1d;
    for _ in 0..500 {
        let domains = random_domains(&mut state);
        let groups = random_groups(&mut state);

        let mut model = Model::<u16>::new();
        for &domain in &domains {
            model.add_var(domain);
        }

        for group in &groups {
            model.add(AllDifferent::new(group.iter().copied()));
        }

        let mut solver = Solver::new(&model);
        let mut solutions = Vec::new();
        solver.for_each_solution(unlimited, |found| {
            solutions.push(found.solution().unwrap());
            ControlFlow::Continue(())
        });

        let mut rules = Rules::new();
        for size in 1..=3 {
            rules.add(NakedSubset::new(size, 0, groups.clone()));
            rules.add(HiddenSubset::new(size, 0, groups.clone()));
        }

        rules.add(Intersection::new(0, groups.clone()));

        let mut pruned = solver.domains().clone();
        match rules.solve(&mut pruned, false) {
            Ok(_) => assert!(
                solutions.iter().all(|solution| keeps16(&pruned, solution)),
                "rules removed a solution of {domains:?} with groups {groups:?}"
            ),
            Err(_) => assert!(solutions.is_empty(), "rules refuted {domains:?} {groups:?}"),
        }
    }
}

/// Returns `true` if every value of `solution` is still in `domains`.
fn keeps16(domains: &Domains<u16>, solution: &[u32]) -> bool {
    solution
        .iter()
        .zip(domains.vars())
        .all(|(&value, var)| domains[var] & (1 << value) != 0)
}
