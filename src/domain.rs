//! The [`Domain`] trait: the set of values a variable may still take, as a bitset.
//!
//! Values are small non-negative integers `0..CAPACITY`. The unsigned primitives `u16`, `u32`,
//! `u64` and `u128` are domains of their bit width, and [`BitSet`] covers wider domains in
//! multiples of 64 values. Pick the smallest type that holds every value: a 9×9 Sudoku fits a
//! `u16`, a 25×25 one a `u32`.

use core::fmt::Debug;

/// The set of values a variable may still take, as a bitset over `0..CAPACITY`.
///
/// Domains are small and `Copy`: the store and the trail copy them by value. All operations
/// are set operations; none of them allocates.
///
/// # Examples
///
/// ```
/// use constraint_search::Domain;
///
/// let mut digits = u16::range(9);
/// digits.remove(4);
/// assert_eq!(digits.len(), 8);
/// assert_eq!(digits.first(), Some(0));
/// assert!(digits.values().eq([0, 1, 2, 3, 5, 6, 7, 8]));
/// ```
pub trait Domain: Copy + Eq + Debug {
    /// The number of values the domain can hold; every value is below it.
    const CAPACITY: u32;

    /// The domain without any value.
    #[must_use]
    fn empty() -> Self;

    /// The domain of the values `0..n`.
    ///
    /// # Panics
    ///
    /// Panics if `n` exceeds [`Domain::CAPACITY`].
    #[must_use]
    fn range(n: u32) -> Self;

    /// The domain holding only `value`.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not below [`Domain::CAPACITY`].
    #[must_use]
    fn singleton(value: u32) -> Self;

    /// Returns `true` if `value` is in the domain. Values at or above the capacity never are.
    #[must_use]
    fn contains(&self, value: u32) -> bool;

    /// Puts `value` into the domain.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not below [`Domain::CAPACITY`].
    fn insert(&mut self, value: u32);

    /// Takes `value` out of the domain. Removing an absent value does nothing.
    fn remove(&mut self, value: u32);

    /// The number of values in the domain.
    #[must_use]
    fn len(&self) -> u32;

    /// The values in `self`, in `other` or in both.
    #[must_use]
    fn union(self, other: Self) -> Self;

    /// The values in both `self` and `other`.
    #[must_use]
    fn intersection(self, other: Self) -> Self;

    /// The values in `self` but not in `other`.
    #[must_use]
    fn difference(self, other: Self) -> Self;

    /// The smallest value, or `None` for the empty domain.
    #[must_use]
    fn first(&self) -> Option<u32>;

    /// Returns `true` if the domain holds no value.
    #[must_use]
    fn is_empty(&self) -> bool {
        *self == Self::empty()
    }

    /// Returns `true` if every value of `self` is also in `other`.
    #[must_use]
    fn is_subset(&self, other: Self) -> bool {
        self.difference(other).is_empty()
    }

    /// The only value, if the domain holds exactly one.
    #[must_use]
    fn value(&self) -> Option<u32> {
        if self.len() == 1 { self.first() } else { None }
    }

    /// The values in ascending order.
    #[must_use]
    fn values(&self) -> Values<Self> {
        Values(*self)
    }
}

/// The values of a domain in ascending order, returned by [`Domain::values`].
#[derive(Debug, Clone)]
pub struct Values<D>(
    /// The values not yet yielded.
    D,
);

impl<D: Domain> Iterator for Values<D> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let value = self.0.first()?;
        self.0.remove(value);

        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.len() as usize;
        (len, Some(len))
    }
}

impl<D: Domain> ExactSizeIterator for Values<D> {}

/// Implements [`Domain`] for unsigned primitives, one bit per value.
macro_rules! primitive_domain {
    ($($bits:ty),*) => {$(
        impl Domain for $bits {
            const CAPACITY: u32 = <$bits>::BITS;

            fn empty() -> Self {
                0
            }

            fn range(n: u32) -> Self {
                assert!(n <= Self::CAPACITY, "{n} values exceed the domain capacity");
                if n == Self::CAPACITY { <$bits>::MAX } else { (1 << n) - 1 }
            }

            fn singleton(value: u32) -> Self {
                assert!(value < Self::CAPACITY, "value {value} exceeds the domain capacity");
                1 << value
            }

            fn contains(&self, value: u32) -> bool {
                value < Self::CAPACITY && (self >> value) & 1 == 1
            }

            fn insert(&mut self, value: u32) {
                *self |= Self::singleton(value);
            }

            fn remove(&mut self, value: u32) {
                if value < Self::CAPACITY {
                    *self &= !(1 << value);
                }
            }

            fn len(&self) -> u32 {
                self.count_ones()
            }

            fn union(self, other: Self) -> Self {
                self | other
            }

            fn intersection(self, other: Self) -> Self {
                self & other
            }

            fn difference(self, other: Self) -> Self {
                self & !other
            }

            fn first(&self) -> Option<u32> {
                (*self != 0).then(|| self.trailing_zeros())
            }
        }
    )*};
}

primitive_domain!(u16, u32, u64, u128);

/// A domain of `64 * W` values, for variables with more values than a `u128` holds.
///
/// # Examples
///
/// ```
/// use constraint_search::{BitSet, Domain};
///
/// let mut slots = BitSet::<4>::range(200);
/// slots.remove(150);
/// assert_eq!(slots.len(), 199);
/// assert!(!slots.contains(150));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BitSet<const W: usize>(
    /// Bit `v % 64` of word `v / 64` is set while the value `v` is in the domain.
    [u64; W],
);

impl<const W: usize> BitSet<W> {
    /// The word index and bit mask of `value`, which must be below the capacity.
    fn locate(value: u32) -> (usize, u64) {
        (value as usize / 64, 1 << (value % 64))
    }

    /// Combines the words of `self` and `other` pairwise with `op`.
    fn zip(self, other: Self, op: impl Fn(u64, u64) -> u64) -> Self {
        let mut words = self.0;
        for (word, other) in words.iter_mut().zip(other.0) {
            *word = op(*word, other);
        }

        Self(words)
    }
}

impl<const W: usize> Domain for BitSet<W> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a domain beyond u32::MAX values would not fit in memory as a bitset"
    )]
    const CAPACITY: u32 = (W * 64) as u32;

    fn empty() -> Self {
        Self([0; W])
    }

    fn range(n: u32) -> Self {
        assert!(n <= Self::CAPACITY, "{n} values exceed the domain capacity");

        let mut words = [0; W];
        let (full, rest) = (n as usize / 64, n % 64);
        words[..full].fill(u64::MAX);
        if rest > 0 {
            words[full] = (1 << rest) - 1;
        }

        Self(words)
    }

    fn singleton(value: u32) -> Self {
        let mut domain = Self::empty();
        domain.insert(value);

        domain
    }

    fn contains(&self, value: u32) -> bool {
        if value >= Self::CAPACITY {
            return false;
        }

        let (word, mask) = Self::locate(value);
        self.0[word] & mask != 0
    }

    fn insert(&mut self, value: u32) {
        assert!(
            value < Self::CAPACITY,
            "value {value} exceeds the domain capacity"
        );

        let (word, mask) = Self::locate(value);
        self.0[word] |= mask;
    }

    fn remove(&mut self, value: u32) {
        if value < Self::CAPACITY {
            let (word, mask) = Self::locate(value);
            self.0[word] &= !mask;
        }
    }

    fn len(&self) -> u32 {
        self.0.iter().map(|word| word.count_ones()).sum()
    }

    fn union(self, other: Self) -> Self {
        self.zip(other, |a, b| a | b)
    }

    fn intersection(self, other: Self) -> Self {
        self.zip(other, |a, b| a & b)
    }

    fn difference(self, other: Self) -> Self {
        self.zip(other, |a, b| a & !b)
    }

    fn first(&self) -> Option<u32> {
        let (index, word) = self.0.iter().enumerate().find(|(_, word)| **word != 0)?;
        let base = u32::try_from(index * 64).expect("the capacity fits in u32");

        Some(base + word.trailing_zeros())
    }
}

/// Unit tests for the primitive domains and [`BitSet`].
#[cfg(test)]
mod tests {
    use super::{BitSet, Domain};

    /// Runs the shared checks on one domain type with at least 64 values.
    fn check_set_algebra<D: Domain>() {
        let low = D::range(10);
        let mut odd = D::empty();
        for value in (1..64).step_by(2) {
            odd.insert(value);
        }

        assert_eq!(low.len(), 10);
        assert_eq!(low.intersection(odd).len(), 5);
        assert_eq!(low.union(odd).len(), 37);
        assert!(low.difference(odd).values().eq([0, 2, 4, 6, 8]));
        assert!(D::singleton(63).is_subset(odd));
        assert_eq!(D::singleton(63).value(), Some(63));
        assert_eq!(low.value(), None);
        assert!(D::empty().is_empty());
        assert_eq!(D::empty().first(), None);
        assert!(!low.contains(D::CAPACITY));
    }

    /// Every primitive wide enough and a two-word [`BitSet`] obey the same set algebra.
    #[test]
    fn set_algebra() {
        check_set_algebra::<u64>();
        check_set_algebra::<u128>();
        check_set_algebra::<BitSet<2>>();
    }

    /// A full-capacity range sets every bit instead of overflowing the shift.
    #[test]
    fn full_range() {
        assert_eq!(u16::range(16), u16::MAX);
        assert_eq!(BitSet::<2>::range(128).len(), 128);
        assert_eq!(BitSet::<2>::range(70).len(), 70);
    }

    /// Removing a value beyond the capacity is a no-op, matching [`Domain::contains`].
    #[test]
    fn remove_out_of_range_is_noop() {
        let mut domain = u16::range(3);
        domain.remove(40);
        assert_eq!(domain, 0b111);
    }

    /// Inserting a value beyond the capacity panics rather than wrapping the shift.
    #[test]
    #[should_panic(expected = "exceeds the domain capacity")]
    fn insert_out_of_range_panics() {
        let mut domain = 0_u16;
        domain.insert(16);
    }
}
