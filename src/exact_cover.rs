//! [`ExactCover`]: pick rows so that every primary column is covered exactly once, by
//! Knuth's Algorithm X with dancing links.
//!
//! Many puzzles are exact cover problems in disguise: Sudoku (each cell, row-digit,
//! column-digit and block-digit exactly once), polyomino tilings, N-queens (with the
//! diagonals as secondary columns, covered at most once).

use alloc::vec::Vec;
use core::ops::ControlFlow;

use crate::Count;

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
/// use constraint_search::ExactCover;
///
/// let mut problem = ExactCover::new(7, 0);
/// for row in [&[2, 4, 5][..], &[0, 3, 6], &[1, 2, 5], &[0, 3], &[1, 6], &[3, 4, 6]] {
///     problem.add_row(row);
/// }
///
/// assert_eq!(problem.solve(), Some(vec![0, 3, 4]));
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

    /// One solution: the chosen rows in ascending order, or `None` if there is none.
    #[must_use]
    pub fn solve(&mut self) -> Option<Vec<usize>> {
        let mut found = None;
        self.for_each_solution(|rows| {
            let mut rows = rows.to_vec();
            rows.sort_unstable();
            found = Some(rows);
            ControlFlow::Break(())
        });

        found
    }

    /// Counts solutions, stopping at `limit`; a limit of 2 decides uniqueness.
    pub fn count(&mut self, limit: u64) -> Count {
        let mut solutions = 0;
        let complete = self.for_each_solution(|_| {
            solutions += 1;
            if solutions >= limit {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });

        if complete {
            Count::Exact(solutions)
        } else {
            Count::AtLeast(solutions)
        }
    }

    /// Calls `on_solution` with the rows of every solution, in the order they were chosen,
    /// until it returns [`ControlFlow::Break`]. Returns `true` if every solution was reported.
    pub fn for_each_solution(
        &mut self,
        mut on_solution: impl FnMut(&[usize]) -> ControlFlow<()>,
    ) -> bool {
        let mut chosen = Vec::new();
        self.search(&mut chosen, &mut on_solution).is_continue()
    }

    /// Algorithm X below the current state, with `chosen` the rows picked so far.
    fn search(
        &mut self,
        chosen: &mut Vec<usize>,
        on_solution: &mut impl FnMut(&[usize]) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        let Some(column) = self.fewest_rows() else {
            return on_solution(chosen);
        };

        self.cover(column);
        let mut node = self.nodes[column].down;
        while node != column {
            chosen.push(self.nodes[node].row);
            self.for_row(node, |this, other| this.cover(this.nodes[other].column));

            let result = self.search(chosen, on_solution);

            self.for_row_back(node, |this, other| this.uncover(this.nodes[other].column));
            chosen.pop();
            if result.is_break() {
                self.uncover(column);
                return result;
            }

            node = self.nodes[node].down;
        }

        self.uncover(column);
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

/// Unit tests for [`ExactCover`].
#[cfg(test)]
mod tests {
    use super::ExactCover;
    use crate::Count;

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
        assert_eq!(queens(6).count(u64::MAX), Count::Exact(4));
        assert_eq!(queens(8).count(u64::MAX), Count::Exact(92));
        assert_eq!(queens(3).count(u64::MAX), Count::Exact(0));
    }

    /// Searching restores the structure, so repeated counts agree, and limits stop early.
    #[test]
    fn search_restores_links() {
        let mut problem = queens(6);
        assert_eq!(problem.count(2), Count::AtLeast(2));
        assert_eq!(problem.count(u64::MAX), Count::Exact(4));
        assert!(problem.solve().is_some());
        assert_eq!(problem.num_rows(), 36);
    }

    /// With no primary column, the empty selection is the one solution.
    #[test]
    fn nothing_to_cover() {
        assert_eq!(ExactCover::new(0, 2).solve(), Some(alloc::vec![]));
    }

    /// A row naming a column twice is rejected.
    #[test]
    #[should_panic(expected = "twice")]
    fn duplicate_column_panics() {
        ExactCover::new(2, 0).add_row(&[1, 1]);
    }
}
