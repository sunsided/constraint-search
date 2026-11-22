//! A brute-force oracle for small exact cover problems, shared by the property tests and the
//! `exact_cover` fuzz target (which includes this file by path).

use constraint_search::{Count, ExactCover, Outcome, Stats};

/// A small exact cover problem in plain data: few enough rows to try every subset.
#[derive(Debug, Clone)]
pub struct Cover {
    /// Columns `0..primary` must be covered exactly once.
    pub primary: usize,
    /// Columns `primary..primary + secondary` may be covered at most once.
    pub secondary: usize,
    /// The columns of each row. Each row lists a column at most once.
    pub rows: Vec<Vec<usize>>,
}

impl Cover {
    /// Checks the count and a solution of [`ExactCover`] against trying every subset of rows.
    pub fn check(&self) {
        let solutions = self.solutions();
        let mut problem = ExactCover::new(self.primary, self.secondary);
        for row in &self.rows {
            problem.add_row(row);
        }

        let expected = u64::try_from(solutions.len()).unwrap();
        let count = problem.count(u64::MAX, unlimited);
        assert_eq!(count.outcome, Count::Exact(expected), "count of {self:?}");
        assert_eq!(count.stats.solutions, expected, "{self:?}");

        match problem.solve(unlimited).outcome {
            Outcome::Solved(rows) => assert!(solutions.contains(&rows), "{rows:?} solves {self:?}"),
            Outcome::Unsatisfiable => assert!(solutions.is_empty(), "nothing found for {self:?}"),
            Outcome::Exhausted => panic!("an unlimited budget never runs out"),
        }

        let paused = problem.count(u64::MAX, |stats: &Stats| stats.nodes >= 2);
        if paused.outcome != Count::Exact(expected) {
            assert_eq!(
                paused.stats.nodes, 2,
                "the budget stops at two nodes for {self:?}"
            );
        }
    }

    /// Every set of rows, ascending, that covers each primary column once and each secondary
    /// column at most once, using only rows with a primary column.
    ///
    /// Duplicate rows yield one solution per copy chosen.
    fn solutions(&self) -> Vec<Vec<usize>> {
        (0..1_u32 << self.rows.len())
            .map(|mask| {
                (0..self.rows.len())
                    .filter(|&row| mask & (1 << row) != 0)
                    .collect::<Vec<_>>()
            })
            .filter(|chosen| chosen.iter().all(|&row| self.has_primary(row)))
            .filter(|chosen| self.covers(chosen))
            .collect()
    }

    /// Returns `true` if `row` covers at least one primary column.
    fn has_primary(&self, row: usize) -> bool {
        self.rows[row].iter().any(|&column| column < self.primary)
    }

    /// Returns `true` if `chosen` covers each primary column once and no column twice.
    fn covers(&self, chosen: &[usize]) -> bool {
        let mut covered = vec![0_u32; self.primary + self.secondary];
        for &row in chosen {
            for &column in &self.rows[row] {
                covered[column] += 1;
            }
        }

        covered[..self.primary].iter().all(|&n| n == 1) && covered.iter().all(|&n| n <= 1)
    }
}

/// A budget that never stops the search.
fn unlimited(_: &Stats) -> bool {
    false
}
