//! Inference rules: named, costed deductions for human-style solving and difficulty grading.
//!
//! Propagators make search fast; rules explain. A [`Rule`] is one technique a person would
//! use ("naked pair", "X-wing"), with a cost that says how hard it is to spot. [`Rules`] keeps
//! them ordered by cost and always applies the cheapest one that makes progress, so a solve
//! reads like a person's, and the most expensive rule it needed grades the puzzle.
//!
//! The crate ships rules for all-different groups: [`VariableSubset`], [`ValueSubset`] and
//! [`GroupIntersection`]. Puzzle-specific patterns implement [`Rule`] themselves.

mod intersection;
mod subset;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Var};
pub use intersection::GroupIntersection;
pub use subset::{ValueSubset, VariableSubset};

/// A named deduction technique with a cost.
///
/// Rules are stateless and `Send + Sync`, like [`Propagator`](crate::Propagator)s. Unlike
/// them, a rule need not catch every consequence; it applies one technique wherever it finds
/// it.
pub trait Rule<D: Domain>: Send + Sync {
    /// The name of the technique, for hints and solve logs.
    fn name(&self) -> &str;

    /// How hard the technique is to apply; rules run cheapest first. The scale is the
    /// caller's: grading reports the highest cost a solve needed.
    fn cost(&self) -> u32;

    /// Returns `true` if the rule is sound only for puzzles with exactly one solution, such as
    /// Sudoku's unique rectangle. [`Rules`] skips these unless asked to assume uniqueness.
    fn assumes_unique(&self) -> bool {
        false
    }

    /// Applies the technique wherever it finds it, removing values from `domains`.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if a deduction would empty a domain: the puzzle has no
    /// solution.
    fn apply(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent>;
}

/// One application of a rule: which rule, and what it removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step<D> {
    /// The rule's position in [`Rules`], for [`Rules::get`].
    pub rule: usize,
    /// The rule's [`Rule::cost`].
    pub cost: u32,
    /// The values removed from each variable, in the order the variables changed. Never
    /// empty; each variable appears once.
    pub removed: Vec<(Var, D)>,
}

/// The steps of a solve by rules alone, and whether they solved the puzzle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace<D> {
    /// The steps in the order they were applied.
    pub steps: Vec<Step<D>>,
    /// `true` if every variable ended with exactly one value.
    pub solved: bool,
}

impl<D> Trace<D> {
    /// The highest cost among the steps, or `None` if the rules could not solve the puzzle.
    ///
    /// A puzzle that was solved before any rule ran grades `Some(0)`.
    #[must_use]
    pub fn grade(&self) -> Option<u32> {
        self.solved
            .then(|| self.steps.iter().map(|step| step.cost).max().unwrap_or(0))
    }
}

/// A set of rules, kept in ascending order of cost.
///
/// Propagators make search fast; rules explain. [`Rules::step`] applies the cheapest rule
/// that makes progress, so a solve reads like a person's and each step can serve as a hint.
/// [`Rules::solve`] repeats that until no rule fires; the most expensive rule it needed
/// grades the puzzle ([`Trace::grade`]).
///
/// ```mermaid
/// flowchart LR
///     start([domains]) --> cheapest[try rules, cheapest first]
///     cheapest -->|a rule removed values| record[record the step] --> cheapest
///     cheapest -->|no rule fires| done([trace: steps, solved?])
/// ```
///
/// # Examples
///
/// ```
/// use constraint_search::{Domain, Domains, ValueSubset, VariableSubset, Rules};
///
/// // One row of four cells over the values 0..4, with two cells already given.
/// let mut domains = Domains::<u16>::new();
/// let row: Vec<_> = (0..4).map(|_| domains.add(u16::range(4))).collect();
/// domains.assign(row[0], 0)?;
/// domains.assign(row[1], 1)?;
///
/// let mut rules = Rules::new();
/// rules.add(VariableSubset::new(1, 1, [row.clone()]));
/// rules.add(ValueSubset::new(1, 2, [row.clone()]));
///
/// let trace = rules.solve(&mut domains, false)?;
/// assert!(!trace.solved); // cells 2 and 3 can still swap values 2 and 3
/// assert_eq!(rules.get(trace.steps[0].rule).name(), "variable single");
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
pub struct Rules<D> {
    /// The rules in ascending order of cost; equal costs keep the order they were added in.
    rules: Vec<Box<dyn Rule<D>>>,
}

impl<D: Domain> Rules<D> {
    /// An empty rule set.
    #[must_use]
    pub const fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Adds `rule` after every rule that costs no more than it.
    pub fn add(&mut self, rule: impl Rule<D> + 'static) -> &mut Self {
        let position = self
            .rules
            .partition_point(|other| other.cost() <= rule.cost());
        self.rules.insert(position, Box::new(rule));
        self
    }

    /// The rule at `index`, as referenced by [`Step::rule`].
    ///
    /// # Panics
    ///
    /// Panics if `index` is not below [`Rules::len`].
    #[must_use]
    pub fn get(&self, index: usize) -> &dyn Rule<D> {
        &*self.rules[index]
    }

    /// The number of rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Returns `true` if the set holds no rule.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Applies the cheapest rule that removes a value, once; `None` if no rule does.
    ///
    /// This is the next hint for a person solving the puzzle. Rules with
    /// [`Rule::assumes_unique`] take part only if `assume_unique` is `true`.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if a rule finds the puzzle has no solution. Values removed by
    /// that rule before it failed stay removed.
    pub fn step(
        &self,
        domains: &mut Domains<D>,
        assume_unique: bool,
    ) -> Result<Option<Step<D>>, Inconsistent> {
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.assumes_unique() && !assume_unique {
                continue;
            }

            let mark = domains.trail_len();
            rule.apply(domains)?;
            if domains.trail_len() > mark {
                return Ok(Some(Step {
                    rule: index,
                    cost: rule.cost(),
                    removed: domains.removed_since(mark),
                }));
            }
        }

        Ok(None)
    }

    /// Applies [`Rules::step`] until no rule makes progress.
    ///
    /// # Errors
    ///
    /// Returns [`Inconsistent`] if a rule finds the puzzle has no solution.
    pub fn solve(
        &self,
        domains: &mut Domains<D>,
        assume_unique: bool,
    ) -> Result<Trace<D>, Inconsistent> {
        let mut steps = Vec::new();
        while let Some(step) = self.step(domains, assume_unique)? {
            steps.push(step);
        }

        Ok(Trace {
            steps,
            solved: domains.is_solved(),
        })
    }
}

impl<D: Domain> Default for Rules<D> {
    fn default() -> Self {
        Self::new()
    }
}

/// Unit tests for [`Rules`] and [`Trace`].
#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::{Rule, Rules, Step, Trace};
    use crate::{Domains, Inconsistent, Var};

    /// Removes value 0 from variable 0, costing `.0`, optionally assuming uniqueness.
    struct DropZero(u32, bool);

    impl Rule<u16> for DropZero {
        fn name(&self) -> &'static str {
            "drop zero"
        }

        fn cost(&self) -> u32 {
            self.0
        }

        fn assumes_unique(&self) -> bool {
            self.1
        }

        fn apply(&self, domains: &mut Domains<u16>) -> Result<(), Inconsistent> {
            domains.remove(Var::new(0), 0).map(|_| ())
        }
    }

    /// Rules sort by cost, stably, and a step names the cheapest rule that fired.
    #[test]
    fn cheapest_rule_fires_first() {
        let mut rules = Rules::new();
        rules
            .add(DropZero(5, false))
            .add(DropZero(2, false))
            .add(DropZero(2, true));
        assert_eq!(rules.get(0).cost(), 2);
        assert!(!rules.get(0).assumes_unique());

        let mut domains = Domains::<u16>::new();
        let x = domains.add(0b11);
        let trace = rules.solve(&mut domains, false).unwrap();

        assert_eq!(
            trace.steps,
            vec![Step {
                rule: 0,
                cost: 2,
                removed: vec![(x, 0b01)]
            }]
        );
        assert_eq!(trace.grade(), Some(2));
    }

    /// A uniqueness rule runs only when asked to assume uniqueness.
    #[test]
    fn uniqueness_rules_are_opt_in() {
        let mut rules = Rules::new();
        rules.add(DropZero(1, true));

        let mut domains = Domains::<u16>::new();
        domains.add(0b11);
        assert_eq!(rules.step(&mut domains, false), Ok(None));
        assert!(rules.step(&mut domains, true).unwrap().is_some());
    }

    /// The set reports its size and hands out its rules.
    #[test]
    fn sizes() {
        let mut rules = Rules::<u16>::default();
        assert!(rules.is_empty());

        rules.add(DropZero(0, false));
        assert_eq!(rules.len(), 1);
        assert_eq!(rules.get(0).name(), "drop zero");
    }

    /// An unsolved trace has no grade; one that needed no step grades zero.
    #[test]
    fn grade_needs_a_solve() {
        let stuck = Trace::<u16> {
            steps: vec![],
            solved: false,
        };
        let given = Trace::<u16> {
            steps: vec![],
            solved: true,
        };
        assert_eq!(stuck.grade(), None);
        assert_eq!(given.grade(), Some(0));
    }
}
