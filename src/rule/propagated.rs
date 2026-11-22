//! [`PropagatorRule`]: any propagator, used as a named and costed inference rule.

use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Propagator, Rule};

/// Runs propagators of one kind as a single [`Rule`], so their deductions get a name and a
/// cost in hints and difficulty grades.
///
/// This turns every constraint into a technique a person can be credited with: sums in
/// Kakuro or Killer cages ([`Linear`](crate::Linear)), row and column clues in nonograms
/// ([`Line`](crate::Line)), allowed combinations ([`Table`](crate::Table)). Each application
/// runs every propagator once, in order.
///
/// # Examples
///
/// ```
/// use constraint_search::{Domain, Domains, Linear, PropagatorRule, Relation, Rules};
///
/// // Minesweeper: a revealed 1 next to two hidden cells, one of which is known safe.
/// let mut domains = Domains::<u16>::new();
/// let safe = domains.add(u16::singleton(0));
/// let hidden = domains.add(u16::range(2));
///
/// let mut rules = Rules::new();
/// let counts = [Linear::sum([safe, hidden], Relation::Equal, 1)];
/// rules.add(PropagatorRule::new("mine count", 1, counts));
///
/// let trace = rules.solve(&mut domains, false)?;
/// assert_eq!(domains.value(hidden), Some(1));
/// assert_eq!(rules.get(trace.steps[0].rule).name(), "mine count");
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropagatorRule<P> {
    /// The rule's [`Rule::name`].
    name: &'static str,
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The propagators, run in order.
    propagators: Vec<P>,
}

impl<P> PropagatorRule<P> {
    /// The rule `name`, costing `cost`, that runs `propagators`.
    #[must_use]
    pub fn new(name: &'static str, cost: u32, propagators: impl IntoIterator<Item = P>) -> Self {
        Self {
            name,
            cost,
            propagators: propagators.into_iter().collect(),
        }
    }
}

impl<D: Domain, P: Propagator<D>> Rule<D> for PropagatorRule<P> {
    fn name(&self) -> &str {
        self.name
    }

    fn cost(&self) -> u32 {
        self.cost
    }

    fn apply(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent> {
        let mut scratch = P::Scratch::default();
        self.propagators
            .iter()
            .try_for_each(|propagator| propagator.propagate(domains, &mut scratch))
    }
}
