//! [`SumDifference`]: subtract one linear equation from another that contains it.

use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Linear, Propagator, Relation, Rule, Var};

/// When every term of equation `A` also appears in equation `B`, the remaining terms of `B`
/// sum to `rhs(B) − rhs(A)`.
///
/// This is how a person combines overlapping sums: in Minesweeper, a 1 touching cells
/// `{a, b}` next to a 2 touching `{a, b, c}` makes `c` a mine (the "1-2 pattern"); in Kakuro
/// and Killer cages, a cage inside a row leaves the rest of the row a known total. Bounds on
/// each sum alone cannot see this. Only [`Relation::Equal`] constraints take part; the
/// derived equations are found once, at construction, and propagate like [`Linear`].
///
/// The default name is "sum difference"; [`SumDifference::with_name`] sets a domain's own
/// term.
///
/// # Examples
///
/// ```
/// use constraint_search::{Domain, Domains, Linear, Relation, Rules, SumDifference};
///
/// let mut domains = Domains::<u16>::new();
/// let [a, b, c] = [(); 3].map(|()| domains.add(u16::range(2)));
/// let one = Linear::sum([a, b], Relation::Equal, 1);
/// let two = Linear::sum([a, b, c], Relation::Equal, 2);
///
/// let mut rules = Rules::new();
/// rules.add(SumDifference::new(2, [one, two]));
/// rules.solve(&mut domains, false)?;
/// assert_eq!(domains.value(c), Some(1));
/// # Ok::<(), constraint_search::Inconsistent>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SumDifference {
    /// The rule's [`Rule::cost`].
    cost: u32,
    /// The rule's [`Rule::name`].
    name: &'static str,
    /// The equations derived by subtracting one given equation from another.
    derived: Vec<Linear>,
}

impl SumDifference {
    /// The rule over `equations`, costing `cost`.
    #[must_use]
    pub fn new(cost: u32, equations: impl IntoIterator<Item = Linear>) -> Self {
        let equations: Vec<Linear> = equations
            .into_iter()
            .filter(|equation| equation.relation() == Relation::Equal)
            .collect();
        let mut derived = Vec::new();
        for (i, inner) in equations.iter().enumerate() {
            for (j, outer) in equations.iter().enumerate() {
                if i != j {
                    derived.extend(difference(inner, outer));
                }
            }
        }

        Self {
            cost,
            name: "sum difference",
            derived,
        }
    }

    /// Names the rule in a domain's own terms, such as "1-2 pattern" for Minesweeper.
    #[must_use]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }
}

/// `outer − inner`, if every term of `inner` appears in `outer` and some term is left over.
fn difference(inner: &Linear, outer: &Linear) -> Option<Linear> {
    let mut rest: Vec<(i64, Var)> = outer.terms().collect();
    for term in inner.terms() {
        let position = rest.iter().position(|&other| other == term)?;
        rest.swap_remove(position);
    }

    (!rest.is_empty()).then(|| Linear::new(rest, Relation::Equal, outer.rhs() - inner.rhs()))
}

impl<D: Domain> Rule<D> for SumDifference {
    fn name(&self) -> &str {
        self.name
    }

    fn cost(&self) -> u32 {
        self.cost
    }

    fn apply(&self, domains: &mut Domains<D>) -> Result<(), Inconsistent> {
        let mut scratch = Vec::new();
        self.derived
            .iter()
            .try_for_each(|equation| equation.propagate(domains, &mut scratch))
    }
}
