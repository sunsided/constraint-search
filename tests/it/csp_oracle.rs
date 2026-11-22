//! A brute-force oracle for small constraint problems, shared by the property tests and the
//! `propagation` fuzz target (which includes this file by path).
//!
//! [`Csp::check`] enumerates every assignment and holds the crate to it: propagation never
//! removes a value of a solution, counting finds exactly the solutions, solving returns one of
//! them, and no rule removes a value of a solution.

use std::ops::ControlFlow;

use constraint_search::{
    AllDifferent, Count, Domain, Domains, GroupIntersection, Line, Linear, Model, Outcome,
    Progress, PropagatorRule, Relation, Rules, Solver, Stats, SumDifference, Table, ValueSubset,
    Var, VariableSubset,
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
    /// `Σ coefficient · value (relation) rhs` over the `(coefficient, variable)` terms.
    Linear(Vec<(i64, usize)>, Relation, i64),
    /// The cells, in order, are 0 or 1 and hold the runs of 1s. Never lists a cell twice.
    Line(Vec<usize>, Vec<usize>),
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
            Self::Linear(terms, relation, rhs) => {
                let sum: i64 = terms
                    .iter()
                    .map(|&(a, var)| a * i64::from(assignment[var]))
                    .sum();
                match relation {
                    Relation::AtMost => sum <= *rhs,
                    Relation::Equal => sum == *rhs,
                    Relation::AtLeast => sum >= *rhs,
                }
            }
            Self::Line(cells, runs) => {
                let values: Vec<u32> = cells.iter().map(|&cell| assignment[cell]).collect();
                runs_of(&values).as_ref() == Some(runs)
            }
        }
    }
}

/// The runs of 1s in `cells`, if every cell is 0 or 1.
fn runs_of(cells: &[u32]) -> Option<Vec<usize>> {
    let mut runs = Vec::new();
    let mut current = 0;
    for &cell in cells {
        match cell {
            1 => current += 1,
            0 => {
                if current > 0 {
                    runs.push(current);
                }

                current = 0;
            }
            _ => return None,
        }
    }

    if current > 0 {
        runs.push(current);
    }

    Some(runs)
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
        self.check_analysis(&model, &solutions);
    }

    /// Possible values, the backbone, marginals and samples agree with the solutions.
    fn check_analysis(&self, model: &Model<u16>, solutions: &[Vec<u32>]) {
        let mut expected = vec![0_u16; self.domains.len()];
        for solution in solutions {
            for (domain, &value) in expected.iter_mut().zip(solution) {
                domain.insert(value);
            }
        }

        let mut solver = Solver::new(model);
        let possible = solver.possible_values(unlimited).outcome;
        assert_eq!(
            possible.as_ref(),
            Some(&expected),
            "possible values of {self:?}"
        );
        let backbone = solver.backbone(unlimited).outcome.unwrap();
        let fixed: Vec<Option<u32>> = expected.iter().map(Domain::value).collect();
        assert_eq!(backbone, fixed, "backbone of {self:?}");

        let marginals = solver.marginals(u64::MAX, unlimited).outcome;
        assert!(marginals.is_exact());
        assert_eq!(
            marginals.solutions(),
            u64::try_from(solutions.len()).unwrap()
        );
        for (index, &domain) in self.domains.iter().enumerate() {
            for value in domain.values() {
                let count = solutions.iter().filter(|s| s[index] == value).count();
                let small = |n: usize| f64::from(u32::try_from(n).unwrap());
                let share = small(count) / small(solutions.len().max(1));
                let probability = marginals.probability(var(index), value);
                assert!((probability - share).abs() < 1e-9, "marginal of {self:?}");
            }
        }

        solver.set_seed(u64::try_from(solutions.len()).unwrap() + 1);
        match solver.sample(3, unlimited).outcome {
            Outcome::Solved(values) => assert!(solutions.contains(&values), "{self:?}"),
            Outcome::Unsatisfiable => assert!(solutions.is_empty(), "{self:?}"),
            Outcome::Exhausted => panic!("an unlimited budget never runs out"),
        }
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
        assert_eq!(
            sliced(&mut solver),
            solutions,
            "sliced solutions of {self:?}"
        );

        #[cfg(feature = "parallel")]
        {
            let parallel = constraint_search::ParallelSolver::new(model, 3);
            let count = parallel.count(u64::MAX, unlimited).outcome;
            assert_eq!(count, Count::Exact(expected), "parallel count of {self:?}");
            match parallel.solve(unlimited).outcome {
                Outcome::Solved(values) => assert!(solutions.contains(&values), "{self:?}"),
                Outcome::Unsatisfiable => assert!(solutions.is_empty(), "{self:?}"),
                Outcome::Exhausted => panic!("an unlimited budget never runs out"),
            }
        }

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
            rules.add(VariableSubset::new(size, 0, groups.clone()));
            rules.add(ValueSubset::new(size, 0, groups.clone()));
        }

        rules.add(GroupIntersection::new(0, groups));
        rules.add(PropagatorRule::new("sums", 0, self.linears()));
        rules.add(SumDifference::new(0, self.linears()));
        rules.add(PropagatorRule::new("lines", 0, self.lines()));
        rules.add(PropagatorRule::new("tables", 0, self.tables()));

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
                Constraint::Table(..) | Constraint::Linear(..) | Constraint::Line(..) => {}
            }
        }

        for table in self.tables() {
            model.add(table);
        }

        for linear in self.linears() {
            model.add(linear);
        }

        for line in self.lines() {
            model.add(line);
        }

        model
    }

    /// The table constraints, as crate constraints.
    fn tables(&self) -> Vec<Table> {
        self.constraints
            .iter()
            .filter_map(|constraint| match constraint {
                Constraint::Table(scope, tuples) => {
                    Some(Table::new(scope.iter().map(|&i| var(i)), tuples))
                }
                _ => None,
            })
            .collect()
    }

    /// The linear constraints, as crate constraints.
    fn linears(&self) -> Vec<Linear> {
        self.constraints
            .iter()
            .filter_map(|constraint| match constraint {
                Constraint::Linear(terms, relation, rhs) => Some(Linear::new(
                    terms.iter().map(|&(a, i)| (a, var(i))),
                    *relation,
                    *rhs,
                )),
                _ => None,
            })
            .collect()
    }

    /// The nonogram lines, as crate constraints.
    fn lines(&self) -> Vec<Line> {
        self.constraints
            .iter()
            .filter_map(|constraint| match constraint {
                Constraint::Line(cells, runs) => {
                    Some(Line::new(cells.iter().map(|&i| var(i)), runs.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The scopes of the all-different constraints, which the rules reason over.
    fn groups(&self) -> Vec<Vec<Var>> {
        self.constraints
            .iter()
            .filter_map(|constraint| match constraint {
                Constraint::AllDifferent(scope) => Some(scope.iter().map(|&i| var(i)).collect()),
                _ => None,
            })
            .collect()
    }
}

/// The crate variable for index `i`.
fn var(i: usize) -> Var {
    Var::new(u32::try_from(i).unwrap())
}

/// Every solution, by a resumable search paused after every two nodes, sorted.
fn sliced(solver: &mut Solver<'_, u16>) -> Vec<Vec<u32>> {
    let mut found = Vec::new();
    solver.begin();
    loop {
        match solver.resume(|stats: &Stats| stats.nodes >= 2).outcome {
            Progress::Solution => found.push(solver.domains().solution().unwrap()),
            Progress::Paused => {}
            Progress::Complete => break,
        }
    }

    assert!(!solver.is_searching());
    found.sort();
    found
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
