//! [`Line`]: a row or column of a nonogram (paint-by-numbers) puzzle.

use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Propagator, Var};

/// The cell value of an empty cell in a [`Line`].
pub const EMPTY: u32 = 0;

/// The cell value of a filled cell in a [`Line`].
pub const FILLED: u32 = 1;

/// A line of cells, each [`EMPTY`] (0) or [`FILLED`] (1), holding the given runs of filled
/// cells in order, separated by at least one empty cell.
///
/// Propagation is complete: it considers every placement of the runs that fits the current
/// domains, and keeps exactly the cell values some placement uses. Values other than 0 and 1
/// are removed. Each run costs `O(n · k · L)` for `n` cells, `k` runs and the longest run `L`.
///
/// # Examples
///
/// A line of five cells with runs 3 and 1 has a single placement:
///
/// ```
/// use constraint_search::{Domain, Domains, Line, Propagator};
///
/// let mut domains = Domains::<u16>::new();
/// let cells: Vec<_> = (0..5).map(|_| domains.add(u16::range(2))).collect();
///
/// Line::new(cells.iter().copied(), [3, 1]).propagate(&mut domains, &mut Default::default())?;
/// let picture: Vec<_> = cells.iter().map(|&c| domains.value(c).unwrap()).collect();
/// assert_eq!(picture, [1, 1, 1, 0, 1]);
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The cells, in order along the line. Never holds a variable twice.
    cells: Vec<Var>,
    /// The lengths of the runs of filled cells, in order. Each at least 1.
    runs: Vec<usize>,
}

impl Line {
    /// The line over `cells` holding `runs` filled cells in order.
    ///
    /// # Panics
    ///
    /// Panics if a run has length 0 or a cell appears twice.
    #[must_use]
    pub fn new(
        cells: impl IntoIterator<Item = Var>,
        runs: impl IntoIterator<Item = usize>,
    ) -> Self {
        let cells: Vec<Var> = cells.into_iter().collect();
        let runs: Vec<usize> = runs.into_iter().collect();
        assert!(
            runs.iter().all(|&run| run > 0),
            "a run needs at least one cell"
        );

        let mut sorted = cells.clone();
        sorted.sort_unstable();
        assert!(
            sorted.windows(2).all(|pair| pair[0] != pair[1]),
            "a line lists a cell twice"
        );

        Self { cells, runs }
    }

    /// The number of states per cell position: one per number of runs placed so far.
    fn width(&self) -> usize {
        self.runs.len() + 1
    }
}

/// Working memory of [`Line`]: reachability of placement states and the values seen.
#[derive(Debug, Default)]
pub struct LineScratch {
    /// `forward[i * w + j]`: the first `i` cells can hold the first `j` runs, with cell `i`
    /// free to start a run.
    forward: Vec<bool>,
    /// `backward[i * w + j]`: from that state, the rest of the line can hold the other runs.
    backward: Vec<bool>,
    /// For each cell, whether some placement leaves it empty and whether one fills it.
    seen: Vec<(bool, bool)>,
}

impl<D: Domain> Propagator<D> for Line {
    type Scratch = LineScratch;

    fn scope(&self) -> &[Var] {
        &self.cells
    }

    fn propagate(
        &self,
        domains: &mut Domains<D>,
        scratch: &mut LineScratch,
    ) -> Result<(), Inconsistent> {
        let open = |cell: usize, value: u32| domains[self.cells[cell]].contains(value);
        self.reach_forward(&open, &mut scratch.forward);
        self.reach_backward(&open, &mut scratch.backward);
        if !scratch.forward[self.cells.len() * self.width() + self.runs.len()] {
            return Err(Inconsistent);
        }

        self.collect_support(&open, scratch);
        for (&cell, &(empty, filled)) in self.cells.iter().zip(&scratch.seen) {
            let mut allowed = D::empty();
            if empty {
                allowed.insert(EMPTY);
            }

            if filled {
                allowed.insert(FILLED);
            }

            domains.restrict(cell, allowed)?;
        }

        Ok(())
    }
}

impl Line {
    /// The state a run `j` placed from cell `i` leads to, if the cells allow it: past the
    /// run and the empty cell after it (or at the end of the line).
    fn place(&self, open: &impl Fn(usize, u32) -> bool, i: usize, j: usize) -> Option<usize> {
        let end = i + *self.runs.get(j)?;
        let n = self.cells.len();
        if end > n || !(i..end).all(|cell| open(cell, FILLED)) {
            return None;
        }

        if end == n {
            Some(n)
        } else {
            open(end, EMPTY).then_some(end + 1)
        }
    }

    /// Marks the states reachable from the start of the line.
    fn reach_forward(&self, open: &impl Fn(usize, u32) -> bool, forward: &mut Vec<bool>) {
        let (n, w) = (self.cells.len(), self.width());
        forward.clear();
        forward.resize((n + 1) * w, false);
        forward[0] = true;

        for i in 0..n {
            for j in 0..w {
                if !forward[i * w + j] {
                    continue;
                }

                if open(i, EMPTY) {
                    forward[(i + 1) * w + j] = true;
                }

                if let Some(next) = self.place(open, i, j) {
                    forward[next * w + j + 1] = true;
                }
            }
        }
    }

    /// Marks the states from which the rest of the line can be completed.
    fn reach_backward(&self, open: &impl Fn(usize, u32) -> bool, backward: &mut Vec<bool>) {
        let (n, w) = (self.cells.len(), self.width());
        backward.clear();
        backward.resize((n + 1) * w, false);
        backward[n * w + self.runs.len()] = true;

        for i in (0..n).rev() {
            for j in 0..w {
                let skip = open(i, EMPTY) && backward[(i + 1) * w + j];
                let run = self
                    .place(open, i, j)
                    .is_some_and(|next| backward[next * w + j + 1]);
                backward[i * w + j] = skip || run;
            }
        }
    }

    /// Records, per cell, the values used by transitions between reachable states that can
    /// still complete the line.
    fn collect_support(&self, open: &impl Fn(usize, u32) -> bool, scratch: &mut LineScratch) {
        let (n, w) = (self.cells.len(), self.width());
        scratch.seen.clear();
        scratch.seen.resize(n, (false, false));

        for i in 0..n {
            for j in 0..w {
                if !scratch.forward[i * w + j] {
                    continue;
                }

                if open(i, EMPTY) && scratch.backward[(i + 1) * w + j] {
                    scratch.seen[i].0 = true;
                }

                let Some(next) = self.place(open, i, j) else {
                    continue;
                };

                if scratch.backward[next * w + j + 1] {
                    let end = i + self.runs[j];
                    scratch.seen[i..end]
                        .iter_mut()
                        .for_each(|cell| cell.1 = true);
                    if end < n {
                        scratch.seen[end].0 = true;
                    }
                }
            }
        }
    }
}

/// Tests [`Line`] against brute-force enumeration.
#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::Line;
    use crate::testing::{Rng, check_against_brute_force};

    /// The runs of filled cells in `cells`, if every cell is 0 or 1.
    fn runs_of(cells: &[u32]) -> Option<Vec<usize>> {
        let mut runs = Vec::new();
        let mut current = 0;
        for &cell in cells {
            match cell {
                1 => current += 1,
                0 if current > 0 => {
                    runs.push(current);
                    current = 0;
                }
                0 => {}
                _ => return None,
            }
        }

        if current > 0 {
            runs.push(current);
        }

        Some(runs)
    }

    /// On random lines and clues, propagation keeps exactly the values some placement uses.
    #[test]
    fn matches_brute_force() {
        let mut rng = Rng::new(31);
        for _ in 0..3000 {
            let cells = 1 + rng.below(7);
            let runs: Vec<usize> = (0..rng.below(3)).map(|_| 1 + rng.below(3)).collect();
            let holds = |assignment: &[u32]| runs_of(assignment).as_ref() == Some(&runs);

            check_against_brute_force(&mut rng, cells, 3, holds, |scope| {
                Line::new(scope, runs.clone())
            });
        }
    }

    /// Runs of length zero are rejected.
    #[test]
    #[should_panic(expected = "at least one cell")]
    fn empty_run_panics() {
        let _ = Line::new([], [0]);
    }
}
