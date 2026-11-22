//! Test support: a small deterministic random source and brute-force checks of propagators.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Domain, Domains, Inconsistent, Propagator, Var};

/// A xorshift64 generator: deterministic, so failing cases reproduce from the seed.
pub(crate) struct Rng(
    /// The generator state. Never zero.
    u64,
);

impl Rng {
    /// A generator seeded with `seed`; a zero seed is replaced, since xorshift sticks at zero.
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    /// The next 64 random bits.
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A random number in `0..bound`, for a non-zero `bound`.
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).expect("below a usize bound")
    }

    /// A random non-empty domain over the values `0..values`.
    pub(crate) fn domain(&mut self, values: u32) -> u16 {
        loop {
            let domain = u16::try_from(self.next() & u64::from(u16::range(values))).unwrap();
            if domain != 0 {
                return domain;
            }
        }
    }
}

/// Builds random domains for `vars` variables over `0..values`, propagates the constraint
/// made by `make` and asserts that exactly the values with a support remain (or that it fails
/// exactly when nothing satisfies `check`).
pub(crate) fn check_against_brute_force<P: Propagator<u16>>(
    rng: &mut Rng,
    vars: usize,
    values: u32,
    check: impl Fn(&[u32]) -> bool,
    make: impl Fn(Vec<Var>) -> P,
) {
    let mut domains = Domains::<u16>::new();
    let scope: Vec<Var> = (0..vars).map(|_| domains.add(rng.domain(values))).collect();
    let before: Vec<u16> = scope.iter().map(|&var| domains[var]).collect();
    let supported = supports(&before, &check);

    let result = make(scope.clone()).propagate(&mut domains, &mut P::Scratch::default());

    if supported.iter().all(|&domain| domain == 0) {
        assert_eq!(result, Err(Inconsistent), "no solution for {before:?}");
    } else {
        assert_eq!(result, Ok(()), "solvable {before:?}");
        let after: Vec<u16> = scope.iter().map(|&var| domains[var]).collect();
        assert_eq!(after, supported, "domains {before:?}");
    }
}

/// For each variable, the values that appear in some assignment from `domains` that passes
/// `check`.
pub(crate) fn supports(domains: &[u16], check: &impl Fn(&[u32]) -> bool) -> Vec<u16> {
    let mut supported = vec![0; domains.len()];
    let mut assignment = vec![0; domains.len()];
    enumerate(domains, &mut assignment, 0, &mut |full| {
        if check(full) {
            for (support, &value) in supported.iter_mut().zip(full) {
                support.insert(value);
            }
        }
    });

    supported
}

/// Calls `visit` with every assignment of `domains[depth..]` appended to
/// `assignment[..depth]`.
fn enumerate(
    domains: &[u16],
    assignment: &mut [u32],
    depth: usize,
    visit: &mut impl FnMut(&[u32]),
) {
    if depth == domains.len() {
        visit(assignment);
        return;
    }

    for value in domains[depth].values() {
        assignment[depth] = value;
        enumerate(domains, assignment, depth + 1, visit);
    }
}

/// Like [`check_against_brute_force`], for propagators that need not be complete: asserts
/// that propagation (run to its own fixpoint, as the engine does) keeps every supported
/// value, fails only when nothing satisfies `check`, and accepts a full assignment only if
/// it satisfies `check`.
pub(crate) fn check_sound_against_brute_force<P: Propagator<u16>>(
    rng: &mut Rng,
    vars: usize,
    values: u32,
    check: impl Fn(&[u32]) -> bool,
    make: impl Fn(Vec<Var>) -> P,
) {
    let mut domains = Domains::<u16>::new();
    let scope: Vec<Var> = (0..vars).map(|_| domains.add(rng.domain(values))).collect();
    let before: Vec<u16> = scope.iter().map(|&var| domains[var]).collect();
    let supported = supports(&before, &check);

    let result = to_fixpoint(&make(scope.clone()), &mut domains, &scope);

    if result.is_err() {
        assert!(
            supported.iter().all(|&d| d == 0),
            "refuted solvable {before:?}"
        );
        return;
    }

    for (&var, &support) in scope.iter().zip(&supported) {
        assert!(
            support.is_subset(domains[var]),
            "removed a solution value of {before:?}"
        );
    }

    let fixed: Option<Vec<u32>> = scope.iter().map(|&var| domains.value(var)).collect();
    if let Some(assignment) = fixed {
        assert!(
            check(&assignment),
            "accepted violating {assignment:?} from {before:?}"
        );
    }
}

/// Runs `propagator` until it changes none of `scope`, as the engine re-runs a propagator
/// that changed its own variables.
fn to_fixpoint<P: Propagator<u16>>(
    propagator: &P,
    domains: &mut Domains<u16>,
    scope: &[Var],
) -> Result<(), Inconsistent> {
    let mut scratch = P::Scratch::default();
    loop {
        let before: Vec<u16> = scope.iter().map(|&var| domains[var]).collect();
        propagator.propagate(domains, &mut scratch)?;
        if scope
            .iter()
            .zip(&before)
            .all(|(&var, &old)| domains[var] == old)
        {
            return Ok(());
        }
    }
}
