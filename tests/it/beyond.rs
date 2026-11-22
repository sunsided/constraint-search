//! Problems beyond all-different grids: a cryptarithm, a nonogram, Minesweeper, card
//! locations and a logic grid, solved by search and explained by rules.

use constraint_search::{
    AllDifferent, Count, Domain, Domains, Line, Linear, Model, Outcome, PropagatorRule, Relation,
    Rules, Solver, Stats, SumDifference, Table, ValueSubset, Var, VariableSubset,
};

use crate::unlimited;

/// SEND + MORE = MONEY: one linear equation with place values, all letters distinct, no
/// leading zeros. It has exactly one solution.
#[test]
#[expect(clippy::many_single_char_names, reason = "the letters of the puzzle")]
fn send_more_money() {
    let mut model = Model::<u16>::new();
    let [s, e, n, d, m, o, r, y] = [(); 8].map(|()| model.add_var(u16::range(10)));
    model.add(AllDifferent::new([s, e, n, d, m, o, r, y]));
    model.add(Linear::new(
        [
            (1000, s),
            (100, e),
            (10, n),
            (1, d),
            (1000, m),
            (100, o),
            (10, r),
            (1, e),
            (-10000, m),
            (-1000, o),
            (-100, n),
            (-10, e),
            (-1, y),
        ],
        Relation::Equal,
        0,
    ));
    model.add(Linear::sum([s], Relation::AtLeast, 1));
    model.add(Linear::sum([m], Relation::AtLeast, 1));

    let mut solver = Solver::new(&model);
    assert_eq!(solver.count(2, unlimited).outcome, Count::Exact(1));
    let Outcome::Solved(digits) = solver.solve(unlimited).outcome else {
        panic!("SEND + MORE = MONEY has a solution")
    };
    assert_eq!(digits, [9, 5, 6, 7, 1, 0, 8, 2]);
}

/// The clues of a 5×5 nonogram whose picture is a plus sign with a dot in each corner.
const ROWS: [&[usize]; 5] = [&[1, 1, 1], &[1], &[5], &[1], &[1, 1, 1]];

/// The column clues of the same picture.
const COLUMNS: [&[usize]; 5] = [&[1, 1, 1], &[1], &[5], &[1], &[1, 1, 1]];

/// The picture, row by row: 1 for filled.
const PICTURE: [[u32; 5]; 5] = [
    [1, 0, 1, 0, 1],
    [0, 0, 1, 0, 0],
    [1, 1, 1, 1, 1],
    [0, 0, 1, 0, 0],
    [1, 0, 1, 0, 1],
];

/// The row and column lines of the nonogram over `cells` (row-major).
fn lines(cells: &[Var]) -> Vec<Line> {
    let rows = (0..5).map(|r| Line::new((0..5).map(|c| cells[r * 5 + c]), ROWS[r].to_vec()));
    let columns = (0..5).map(|c| Line::new((0..5).map(|r| cells[r * 5 + c]), COLUMNS[c].to_vec()));

    rows.chain(columns).collect()
}

/// The nonogram has one solution by search, and line-by-line reasoning alone solves it.
#[test]
fn nonogram_by_search_and_by_rules() {
    let mut model = Model::<u16>::new();
    let cells = model.add_vars(25, u16::range(2));
    for line in lines(&cells) {
        model.add(line);
    }

    let mut solver = Solver::new(&model);
    assert_eq!(solver.count(2, unlimited).outcome, Count::Exact(1));

    let mut domains = Domains::<u16>::new();
    let cells: Vec<Var> = (0..25).map(|_| domains.add(u16::range(2))).collect();
    let mut rules = Rules::new();
    rules.add(PropagatorRule::new("line", 1, lines(&cells)));

    let trace = rules.solve(&mut domains, false).unwrap();
    assert!(trace.solved);
    assert_eq!(trace.grade(), Some(1));
    let picture: Vec<u32> = cells.iter().map(|&c| domains.value(c).unwrap()).collect();
    assert_eq!(picture, PICTURE.concat());
}

/// Minesweeper: a revealed 1 touching two hidden cells next to a revealed 2 touching those
/// two and a third. Counting shows which cell is safe.
#[test]
fn minesweeper_counts() {
    let mut domains = Domains::<u16>::new();
    let [a, b, c] = [(); 3].map(|()| domains.add(u16::range(2)));

    let counts = [
        Linear::sum([a, b], Relation::Equal, 1),
        Linear::sum([a, b, c], Relation::Equal, 2),
    ];
    let mut rules = Rules::new();
    rules.add(PropagatorRule::new("mine count", 1, counts.clone()));
    rules.add(SumDifference::new(2, counts).with_name("1-2 pattern"));

    let trace = rules.solve(&mut domains, false).unwrap();
    assert_eq!(domains.value(c), Some(1), "c is a mine");
    assert!(!trace.solved, "a and b stay a coin flip");
    let step = trace.why_removed(c, 0).expect("c can no longer be safe");
    assert_eq!(rules.get(step.rule).name(), "1-2 pattern");
    assert_eq!(trace.grade(), None);
}

/// Card locations: four spades among three hands. North and East are void in spades, so
/// only West can hold them, and West's hand of four is then full.
#[test]
fn only_west_can_hold_the_spades() {
    const HANDS: usize = 3;
    const WEST: usize = 0;
    let mut domains = Domains::<u16>::new();
    // holds[card][hand] is 1 if that hand holds that card; cards 0..4 are spades.
    let holds: Vec<[Var; HANDS]> = (0..6)
        .map(|_| [(); HANDS].map(|()| domains.add(u16::range(2))))
        .collect();
    for spade in &holds[..4] {
        for &void in &spade[1..] {
            domains.assign(void, 0).unwrap();
        }
    }

    let each_card_once = holds
        .iter()
        .map(|hands| Linear::sum(*hands, Relation::Equal, 1));
    let hand_sizes = [4, 1, 1].into_iter().enumerate().map(|(hand, size)| {
        Linear::sum(holds.iter().map(|hands| hands[hand]), Relation::Equal, size)
    });
    let mut rules = Rules::new();
    rules.add(PropagatorRule::new(
        "card counting",
        1,
        each_card_once.chain(hand_sizes),
    ));

    let trace = rules.solve(&mut domains, false).unwrap();
    for spade in &holds[..4] {
        assert_eq!(domains.value(spade[WEST]), Some(1));
    }

    for other in &holds[4..] {
        assert_eq!(domains.value(other[WEST]), Some(0), "West's hand is full");
        assert!(trace.why_removed(other[WEST], 1).is_some());
    }
}

/// A logic-grid puzzle: three houses, a colour and a pet each. The red house is first, the
/// dog lives in the green house, the cat in the third house, and the blue house is not the
/// second. Each attribute is a permutation of the houses, so subset rules apply directly.
#[test]
fn logic_grid_by_rules() {
    let mut domains = Domains::<u16>::new();
    // Each variable is the house (0, 1 or 2) that has the attribute.
    let [red, green, blue, cat, dog, fish] = [(); 6].map(|()| domains.add(u16::range(3)));
    domains.assign(red, 0).unwrap();
    domains.assign(cat, 2).unwrap();
    domains.remove(blue, 1).unwrap();

    let colours = [red, green, blue];
    let pets = [cat, dog, fish];
    let same_house = Table::new([dog, green], [[0, 0], [1, 1], [2, 2]]);
    let mut rules = Rules::new();
    rules
        .add(VariableSubset::new(1, 1, [colours, pets]))
        .add(ValueSubset::new(1, 2, [colours, pets]))
        .add(PropagatorRule::new("same house", 3, [same_house]));

    let trace = rules.solve(&mut domains, false).unwrap();
    assert!(trace.solved);
    let houses = [green, blue, dog, fish].map(|var| domains.value(var).unwrap());
    assert_eq!(houses, [1, 2, 1, 0]);
    assert_eq!(trace.grade(), Some(3), "the clue about the dog was needed");
}

/// The 1-2 pattern as a model: the backbone holds the certain mine, marginals the odds of
/// the others, and a weighted prior shifts those odds.
#[test]
fn minesweeper_backbone_and_odds() {
    let mut model = Model::<u16>::new();
    let [a, b, c] = [(); 3].map(|()| model.add_var(u16::range(2)));
    model.add(Linear::sum([a, b], Relation::Equal, 1));
    model.add(Linear::sum([a, b, c], Relation::Equal, 2));
    let mut solver = Solver::new(&model);

    let backbone = solver.backbone(unlimited).outcome.unwrap();
    assert_eq!(backbone, [None, None, Some(1)], "only c is certain");

    let odds = solver.marginals(u64::MAX, unlimited).outcome;
    assert!(odds.is_exact());
    assert_eq!(odds.solutions(), 2);
    assert!((odds.probability(a, 1) - 0.5).abs() < 1e-12);
    assert!((odds.probability(c, 1) - 1.0).abs() < 1e-12);

    let prior = solver.weighted_marginals(u64::MAX, unlimited, |domains| {
        if domains.value(a) == Some(1) {
            3.0
        } else {
            1.0
        }
    });
    assert!((prior.outcome.probability(a, 1) - 0.75).abs() < 1e-12);
    assert!((prior.outcome.total_weight() - 4.0).abs() < 1e-12);
}

/// Budgets and limits leave the analysis incomplete, and samples repeat with their seed.
#[test]
fn analysis_limits_and_seeded_samples() {
    let mut model = Model::<u16>::new();
    let vars = model.add_vars(5, u16::range(5));
    model.add(AllDifferent::new(vars.iter().copied()));
    let mut solver = Solver::new(&model);

    assert_eq!(
        solver
            .possible_values(|stats: &Stats| stats.nodes >= 3)
            .outcome,
        None
    );
    assert_eq!(
        solver.sample(4, |_: &Stats| true).outcome,
        Outcome::Exhausted
    );
    let partial = solver.marginals(10, unlimited).outcome;
    assert!(!partial.is_exact());
    assert_eq!(partial.solutions(), 10);
    assert_eq!(solver.marginals(0, unlimited).outcome.solutions(), 0);

    let draws: Vec<Outcome> = (0..2)
        .map(|_| {
            solver.set_seed(42);
            solver.sample(1, unlimited).outcome
        })
        .collect();
    assert_eq!(draws[0], draws[1], "the same seed gives the same sample");

    solver.set_seed(42);
    let distinct: std::collections::HashSet<_> = (0..20)
        .map(|_| format!("{:?}", solver.sample(1, unlimited).outcome))
        .collect();
    assert!(distinct.len() > 1, "successive samples differ");
}

/// The analysis answers for the domains a paused search began from, never for that search's
/// narrowed state, and its edge cases are defined.
#[test]
fn analysis_after_a_paused_search() {
    let mut model = Model::<u16>::new();
    let vars = model.add_vars(3, u16::range(3));
    model.add(AllDifferent::new(vars.iter().copied()));
    let mut solver = Solver::new(&model);

    solver.begin();
    assert_eq!(
        solver.resume(unlimited).outcome,
        constraint_search::Progress::Solution
    );
    let possible = solver.possible_values(unlimited).outcome.unwrap();
    assert!(
        possible.iter().all(|&domain| domain == 0b111),
        "every value is possible"
    );
    assert_eq!(
        solver.backbone(unlimited).outcome.unwrap(),
        [None, None, None]
    );

    solver.begin();
    let _ = solver.resume(unlimited);
    let odds = solver.marginals(u64::MAX, unlimited).outcome;
    assert_eq!(odds.solutions(), 6);
    assert!((odds.probability(vars[2], 2) - 1.0 / 3.0).abs() < 1e-12);

    let at_limit = solver.marginals(6, unlimited).outcome;
    assert!(
        at_limit.is_exact(),
        "six solutions under a limit of six are all of them"
    );
    assert!(!solver.marginals(5, unlimited).outcome.is_exact());

    solver.begin();
    assert_eq!(solver.sample(0, unlimited).outcome, Outcome::Exhausted);
    assert!(!solver.is_searching());
}
