//! [`ExactCover`]: pick rows so that every primary column is covered exactly once, by
//! Knuth's Algorithm X with dancing links.
//!
//! Many puzzles are exact cover problems in disguise: Sudoku (each cell, row-digit,
//! column-digit and block-digit exactly once), polyomino tilings, N-queens (with the
//! diagonals as secondary columns, covered at most once).

use alloc::vec::Vec;
use core::ops::ControlFlow;

use search_core::Budget;

use crate::{Count, Outcome, Report, Stats, Status};

/// An exact cover problem: columns to cover and rows that each cover some of them.
///
/// Primary columns must be covered exactly once, secondary columns at most once. Search
/// branches on the primary column with the fewest rows left, so a row without a primary
/// column is never chosen. The links are indices into flat arrays, so the structure needs no
/// unsafe code and searches in place, restoring itself afterwards.
///
/// # Examples
///
/// Knuth's example from the dancing links paper, with its single solution:
///
/// ```
/// use constraint_search::{ExactCover, Outcome, Stats};
///
/// let mut problem = ExactCover::new(7, 0);
/// for row in [&[2, 4, 5][..], &[0, 3, 6], &[1, 2, 5], &[0, 3], &[1, 6], &[3, 4, 6]] {
///     problem.add_row(row);
/// }
///
/// let report = problem.solve(|_: &Stats| false);
/// assert_eq!(report.outcome, Outcome::Solved(vec![0, 3, 4]));
/// ```
#[derive(Debug, Clone)]
pub struct ExactCover {
    /// Every node: the root (node 0), the column headers (nodes `1..=columns`), then the row
    /// nodes in row order. Primary headers are linked in a ring with the root; secondary
    /// headers link only to themselves.
    nodes: Vec<Node>,
    /// The number of rows still covering each column, indexed by header node.
    sizes: Vec<usize>,
    /// The number of rows added.
    rows: usize,
    /// The rows chosen so far by the running search; kept for its capacity between searches.
    chosen: Vec<usize>,
}

/// One node of the four-way linked structure.
#[derive(Debug, Clone, Copy)]
struct Node {
    /// The node to the left: the previous column header, or the previous node of the row.
    left: usize,
    /// The node to the right.
    right: usize,
    /// The node above in the column.
    up: usize,
    /// The node below in the column.
    down: usize,
    /// The header node of the column.
    column: usize,
    /// The row of a row node; unused for the root and headers.
    row: usize,
}

impl ExactCover {
    /// A problem with `primary` columns to cover exactly once, followed by `secondary`
    /// columns to cover at most once. Columns are numbered from 0, primary ones first.
    #[must_use]
    pub fn new(primary: usize, secondary: usize) -> Self {
        let columns = primary + secondary;
        let mut nodes = Vec::with_capacity(columns + 1);
        for index in 0..=columns {
            let linked = index <= primary;
            nodes.push(Node {
                left: if linked {
                    (index + primary) % (primary + 1)
                } else {
                    index
                },
                right: if linked {
                    (index + 1) % (primary + 1)
                } else {
                    index
                },
                up: index,
                down: index,
                column: index,
                row: usize::MAX,
            });
        }

        Self {
            sizes: alloc::vec![0; columns + 1],
            nodes,
            rows: 0,
            chosen: Vec::new(),
        }
    }

    /// Adds a row covering `columns`; returns its index, counting from 0.
    ///
    /// # Panics
    ///
    /// Panics if a column is out of range or listed twice.
    pub fn add_row(&mut self, columns: &[usize]) -> usize {
        let count = self.sizes.len() - 1;
        let mut sorted = columns.to_vec();
        sorted.sort_unstable();
        assert!(
            sorted.iter().all(|&column| column < count),
            "column out of range"
        );
        assert!(
            sorted.windows(2).all(|pair| pair[0] != pair[1]),
            "a row lists a column twice"
        );

        let row = self.rows;
        let first = self.nodes.len();
        for (offset, &column) in columns.iter().enumerate() {
            let header = column + 1;
            let index = first + offset;
            let up = self.nodes[header].up;
            self.nodes.push(Node {
                left: if offset == 0 {
                    first + columns.len() - 1
                } else {
                    index - 1
                },
                right: if offset + 1 == columns.len() {
                    first
                } else {
                    index + 1
                },
                up,
                down: header,
                column: header,
                row,
            });
            self.nodes[up].down = index;
            self.nodes[header].up = index;
            self.sizes[header] += 1;
        }

        self.rows += 1;
        row
    }

    /// The number of rows added.
    #[must_use]
    pub fn num_rows(&self) -> usize {
        self.rows
    }

    /// One solution: the chosen rows in ascending order.
    pub fn solve(&mut self, budget: impl Budget<Stats>) -> Report<Outcome<Vec<usize>>> {
        let mut found = None;
        let report = self.for_each_solution(budget, |rows| {
            let mut rows = rows.to_vec();
            rows.sort_unstable();
            found = Some(rows);
            ControlFlow::Break(())
        });

        let outcome = match (report.outcome, found) {
            (_, Some(rows)) => Outcome::Solved(rows),
            (Status::Exhausted, None) => Outcome::Exhausted,
            (_, None) => Outcome::Unsatisfiable,
        };

        Report {
            outcome,
            stats: report.stats,
        }
    }

    /// Counts solutions, stopping at `limit`; a limit of 2 decides uniqueness. A limit of 0
    /// searches nothing and reports `Count::AtLeast(0)`.
    pub fn count(&mut self, limit: u64, budget: impl Budget<Stats>) -> Report<Count> {
        if limit == 0 {
            return Report {
                outcome: Count::AtLeast(0),
                stats: Stats::default(),
            };
        }

        let mut solutions = 0;
        let report = self.for_each_solution(budget, |_| {
            solutions += 1;
            if solutions >= limit {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });

        let outcome = match report.outcome {
            Status::Complete => Count::Exact(solutions),
            Status::Stopped | Status::Exhausted => Count::AtLeast(solutions),
        };

        Report {
            outcome,
            stats: report.stats,
        }
    }

    /// Calls `on_solution` with the rows of every solution, in the order they were chosen,
    /// until it returns [`ControlFlow::Break`], the budget stops the search or the search
    /// ends.
    ///
    /// The budget is consulted before each search node. [`Stats::failures`] counts dead ends:
    /// a primary column that no remaining row covers. Searching restores the links, and
    /// reuses its buffers across calls.
    pub fn for_each_solution(
        &mut self,
        mut budget: impl Budget<Stats>,
        mut on_solution: impl FnMut(&[usize]) -> ControlFlow<()>,
    ) -> Report<Status> {
        let mut chosen = core::mem::take(&mut self.chosen);
        chosen.clear();
        let mut search = Search {
            budget: &mut budget,
            on_solution: &mut on_solution,
            chosen: &mut chosen,
            stats: Stats::default(),
        };

        let outcome = match self.search(&mut search) {
            ControlFlow::Continue(()) => Status::Complete,
            ControlFlow::Break(Stop::Caller) => Status::Stopped,
            ControlFlow::Break(Stop::Budget) => Status::Exhausted,
        };

        let stats = search.stats;
        self.chosen = chosen;
        Report { outcome, stats }
    }

    /// Algorithm X below the current state.
    fn search<B, F>(&mut self, search: &mut Search<'_, B, F>) -> ControlFlow<Stop>
    where
        B: Budget<Stats>,
        F: FnMut(&[usize]) -> ControlFlow<()>,
    {
        if search.budget.exhausted(&search.stats) {
            return ControlFlow::Break(Stop::Budget);
        }

        search.stats.nodes += 1;
        let Some(column) = self.fewest_rows() else {
            search.stats.solutions += 1;
            return (search.on_solution)(search.chosen).map_break(|()| Stop::Caller);
        };

        if self.sizes[column] == 0 {
            search.stats.failures += 1;
            return ControlFlow::Continue(());
        }

        self.cover(column);
        let result = self.try_rows(column, search);
        self.uncover(column);

        result
    }

    /// Tries each row covering the already covered `column`, searching below each.
    fn try_rows<B, F>(&mut self, column: usize, search: &mut Search<'_, B, F>) -> ControlFlow<Stop>
    where
        B: Budget<Stats>,
        F: FnMut(&[usize]) -> ControlFlow<()>,
    {
        let mut node = self.nodes[column].down;
        while node != column {
            search.chosen.push(self.nodes[node].row);
            self.for_row(node, |this, other| this.cover(this.nodes[other].column));

            let result = self.search(search);

            self.for_row_back(node, |this, other| this.uncover(this.nodes[other].column));
            search.chosen.pop();
            result?;

            node = self.nodes[node].down;
        }

        ControlFlow::Continue(())
    }

    /// The uncovered primary column with the fewest rows, or `None` if all are covered.
    fn fewest_rows(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut column = self.nodes[0].right;
        while column != 0 {
            if best.is_none_or(|best| self.sizes[column] < self.sizes[best]) {
                best = Some(column);
            }

            column = self.nodes[column].right;
        }

        best
    }

    /// Calls `visit` with every other node of `node`'s row, left to right.
    fn for_row(&mut self, node: usize, mut visit: impl FnMut(&mut Self, usize)) {
        let mut other = self.nodes[node].right;
        while other != node {
            visit(self, other);
            other = self.nodes[other].right;
        }
    }

    /// Calls `visit` with every other node of `node`'s row, right to left.
    fn for_row_back(&mut self, node: usize, mut visit: impl FnMut(&mut Self, usize)) {
        let mut other = self.nodes[node].left;
        while other != node {
            visit(self, other);
            other = self.nodes[other].left;
        }
    }

    /// Removes `column` from the header ring and every row covering it from the other
    /// columns.
    fn cover(&mut self, column: usize) {
        let Node { left, right, .. } = self.nodes[column];
        self.nodes[left].right = right;
        self.nodes[right].left = left;

        let mut row = self.nodes[column].down;
        while row != column {
            self.for_row(row, |this, node| {
                let Node {
                    up, down, column, ..
                } = this.nodes[node];
                this.nodes[up].down = down;
                this.nodes[down].up = up;
                this.sizes[column] -= 1;
            });
            row = self.nodes[row].down;
        }
    }

    /// Undoes [`ExactCover::cover`] of `column`, in exactly reverse order.
    fn uncover(&mut self, column: usize) {
        let mut row = self.nodes[column].up;
        while row != column {
            self.for_row_back(row, |this, node| {
                let Node {
                    up, down, column, ..
                } = this.nodes[node];
                this.nodes[up].down = node;
                this.nodes[down].up = node;
                this.sizes[column] += 1;
            });
            row = self.nodes[row].up;
        }

        let Node { left, right, .. } = self.nodes[column];
        self.nodes[left].right = column;
        self.nodes[right].left = column;
    }
}

/// Why a search stopped before exhausting its space.
enum Stop {
    /// The solution callback asked to stop.
    Caller,
    /// The budget ran out.
    Budget,
}

/// One running search: its budget, callback, chosen rows and counters.
struct Search<'s, B, F> {
    /// Decides when to stop early.
    budget: &'s mut B,
    /// Receives each solution.
    on_solution: &'s mut F,
    /// The rows chosen on the path to the current node.
    chosen: &'s mut Vec<usize>,
    /// The counters so far.
    stats: Stats,
}

/// Unit tests for [`ExactCover`].
#[cfg(test)]
mod tests {
    use super::ExactCover;
    use crate::{Count, Outcome, Stats};

    /// A budget that never stops the search.
    fn unlimited(_: &Stats) -> bool {
        false
    }

    /// N-queens with rows and columns primary and diagonals secondary.
    fn queens(n: usize) -> ExactCover {
        let diagonals = 2 * n - 1;
        let mut problem = ExactCover::new(2 * n, 2 * diagonals);
        for row in 0..n {
            for column in 0..n {
                let rising = 2 * n + row + column;
                let falling = 2 * n + diagonals + row + n - 1 - column;
                problem.add_row(&[row, n + column, rising, falling]);
            }
        }

        problem
    }

    /// The classic queens counts, which exercise secondary columns.
    #[test]
    fn queens_counts() {
        assert_eq!(
            queens(6).count(u64::MAX, unlimited).outcome,
            Count::Exact(4)
        );
        assert_eq!(
            queens(8).count(u64::MAX, unlimited).outcome,
            Count::Exact(92)
        );
        assert_eq!(
            queens(3).count(u64::MAX, unlimited).outcome,
            Count::Exact(0)
        );
    }

    /// Searching restores the structure, so repeated counts agree, and limits stop early.
    #[test]
    fn search_restores_links() {
        let mut problem = queens(6);
        assert_eq!(problem.count(0, unlimited).outcome, Count::AtLeast(0));
        assert_eq!(problem.count(2, unlimited).outcome, Count::AtLeast(2));
        assert_eq!(problem.count(u64::MAX, unlimited).outcome, Count::Exact(4));
        assert!(matches!(
            problem.solve(unlimited).outcome,
            Outcome::Solved(_)
        ));
        assert_eq!(problem.num_rows(), 36);
    }

    /// A budget stops the search after the nodes it allows, and the links are restored.
    #[test]
    fn budget_stops_the_search() {
        let mut problem = queens(8);
        let tight = |stats: &Stats| stats.nodes >= 10;

        let count = problem.count(u64::MAX, tight);
        assert!(matches!(count.outcome, Count::AtLeast(_)));
        assert_eq!(count.stats.nodes, 10);
        assert_eq!(problem.solve(|_: &Stats| true).outcome, Outcome::Exhausted);
        assert!(queens(3).count(u64::MAX, unlimited).stats.failures > 0);
        assert_eq!(problem.count(u64::MAX, unlimited).outcome, Count::Exact(92));
    }

    /// With no primary column, the empty selection is the one solution.
    #[test]
    fn nothing_to_cover() {
        assert_eq!(
            ExactCover::new(0, 2).solve(unlimited).outcome,
            Outcome::Solved(alloc::vec![])
        );
    }

    /// A row naming a column twice is rejected.
    #[test]
    #[should_panic(expected = "twice")]
    fn duplicate_column_panics() {
        ExactCover::new(2, 0).add_row(&[1, 1]);
    }
}
