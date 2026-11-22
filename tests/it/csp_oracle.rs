//! A brute-force oracle for small constraint problems, shared by the property tests and the
//! `propagation` fuzz target (which includes this file by path).
//!
//! [`Csp::check`] enumerates every assignment and holds the crate to it: propagation never
//! removes a value of a solution, counting finds exactly the solutions, solving returns one of
//! them, and no rule removes a value of a solution.

use std::ops::ControlFlow;

use constraint_search::{
    AllDifferent, Count, Domain, Domains, HiddenSubset, Intersection, Model, NakedSubset, Outcome,
    Rules, Solver, Stats, Table, Var,
};

/// The number of values each variable may take at most: domains are subsets of `0..VALUES`.
pub const VALUES: u32 = 6;

/// One constraint of a [`Csp`], over variable indices.
#[derive(Debug, Clone)]
pub enum Constraint {
    /// The variables take pairwise distinct values. Never lists a variable twice.
    AllDifferent(Vec<usize>),
    /// The variables take one of the tuples, each one value per variable. Never lists a
    /// variable twice.
    Table(Vec<usize>, Vec<Vec<u32>>),
}

impl Constraint {
    /// Returns `true` if `assignment` (one value per variable of the problem) satisfies the
    /// constraint.
    fn holds(&self, assignment: &[u32]) -> bool {
        match self {
            Self::AllDifferent(vars) => vars.iter().enumerate().all(|(i, &a)| {
                vars[i + 1..]
                    .iter()
                    .all(|&b| assignment[a] != assignment[b])
            }),
            Self::Table(vars, tuples) => tuples.iter().any(|tuple| {
                vars.iter()
                    .zip(tuple)
                    .all(|(&var, &v)| assignment[var] == v)
            }),
        }
    }
}

/// A small constraint problem in plain data, so an oracle can enumerate it.
#[derive(Debug, Clone)]
pub struct Csp {
    /// The initial domain of each variable, as a bitset over `0..VALUES`. Never empty.
    pub domains: Vec<u16>,
    /// The constraints.
    pub constraints: Vec<Constraint>,
}

impl Csp {
    /// Runs every check against brute-force enumeration, panicking on the first mismatch.
    pub fn check(&self) {
        let solutions = self.solutions();
        let model = self.model();

        self.check_propagation(&model, &solutions);
        self.check_search(&model, &solutions);
        self.check_rules(&solutions);
    }

    /// Propagation fails only without solutions, and keeps every value of every solution.
    fn check_propagation(&self, model: &Model<u16>, solutions: &[Vec<u32>]) {
        let mut solver = Solver::new(model);
        match solver.propagate() {
            Ok(()) => assert!(
                solutions.iter().all(|s| keeps(solver.domains(), s)),
                "propagation removed a solution of {self:?}"
            ),
            Err(_) => assert!(solutions.is_empty(), "propagation refuted {self:?}"),
        }
    }

    /// Counting finds exactly the solutions, enumeration lists each once, and solving
    /// returns one of them.
    fn check_search(&self, model: &Model<u16>, solutions: &[Vec<u32>]) {
        let mut solver = Solver::new(model);
        let expected = u64::try_from(solutions.len()).unwrap();
        let count = solver.count(u64::MAX, unlimited).outcome;
        assert_eq!(count, Count::Exact(expected), "count of {self:?}");

        let mut found = Vec::new();
        solver.for_each_solution(unlimited, |domains| {
            found.push(domains.solution().unwrap());
            ControlFlow::Continue(())
        });
        found.sort();
        assert_eq!(found, solutions, "solutions of {self:?}");

        match solver.solve(unlimited).outcome {
            Outcome::Solved(values) => assert!(solutions.contains(&values), "{self:?}"),
            Outcome::Unsatisfiable => assert!(solutions.is_empty(), "{self:?}"),
            Outcome::Exhausted => panic!("an unlimited budget never runs out"),
        }
    }

    /// The rules over the all-different groups keep every value of every solution.
    fn check_rules(&self, solutions: &[Vec<u32>]) {
        let groups = self.groups();
        let mut rules = Rules::new();
        for size in 1..=3 {
            rules.add(NakedSubset::new(size, 0, groups.clone()));
            rules.add(HiddenSubset::new(size, 0, groups.clone()));
        }

        rules.add(Intersection::new(0, groups));

        let mut domains = Domains::new();
        for &domain in &self.domains {
            domains.add(domain);
        }

        match rules.solve(&mut domains, false) {
            Ok(_) => assert!(
                solutions.iter().all(|s| keeps(&domains, s)),
                "a rule removed a solution of {self:?}"
            ),
            Err(_) => assert!(solutions.is_empty(), "the rules refuted {self:?}"),
        }
    }

    /// Every satisfying assignment, in lexicographic order.
    pub fn solutions(&self) -> Vec<Vec<u32>> {
        let mut solutions = Vec::new();
        let mut assignment = vec![0; self.domains.len()];
        self.enumerate(&mut assignment, 0, &mut solutions);

        solutions
    }

    /// Extends `assignment[..depth]` in every way and collects the satisfying assignments.
    fn enumerate(&self, assignment: &mut [u32], depth: usize, solutions: &mut Vec<Vec<u32>>) {
        if depth == assignment.len() {
            if self.constraints.iter().all(|c| c.holds(assignment)) {
                solutions.push(assignment.to_vec());
            }

            return;
        }

        for value in self.domains[depth].values() {
            assignment[depth] = value;
            self.enumerate(assignment, depth + 1, solutions);
        }
    }

    /// The problem as a crate model.
    fn model(&self) -> Model<u16> {
        let mut model = Model::new();
        let vars: Vec<Var> = self.domains.iter().map(|&d| model.add_var(d)).collect();
        for constraint in &self.constraints {
            match constraint {
                Constraint::AllDifferent(scope) => {
                    model.add(AllDifferent::new(scope.iter().map(|&i| vars[i])));
                }
                Constraint::Table(scope, tuples) => {
                    model.add(Table::new(scope.iter().map(|&i| vars[i]), tuples));
                }
            }
        }

        model
    }

    /// The scopes of the all-different constraints, which the rules reason over.
    fn groups(&self) -> Vec<Vec<Var>> {
        let var = |i: &usize| Var::new(u32::try_from(*i).unwrap());
        self.constraints
            .iter()
            .filter_map(|constraint| match constraint {
                Constraint::AllDifferent(scope) => Some(scope.iter().map(var).collect()),
                Constraint::Table(..) => None,
            })
            .collect()
    }
}

/// A budget that never stops the search.
fn unlimited(_: &Stats) -> bool {
    false
}

/// Returns `true` if every value of `solution` is still in `domains`.
fn keeps(domains: &Domains<u16>, solution: &[u32]) -> bool {
    solution
        .iter()
        .zip(domains.vars())
        .all(|(&value, var)| domains[var].contains(value))
}
