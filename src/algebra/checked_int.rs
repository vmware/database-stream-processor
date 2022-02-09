use crate::algebra::{AddAssignByRef, AddByRef, HasOne, HasZero, MulByRef, NegByRef};
use num::{traits::CheckedNeg, CheckedAdd, CheckedMul};
use std::{
    cmp::Ordering,
    fmt::{Debug, Display, Error, Formatter},
    ops::{Add, AddAssign, Neg},
};

/// Ring on numeric values that panics on overflow
/// Computes exactly like any signed numeric value, but panics on overflow
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct CheckedInt<T> {
    value: T,
}

impl<T> CheckedInt<T> {
    fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T> Add for CheckedInt<T>
where
    T: CheckedAdd,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        // intentional panic on overflow
        Self {
            value: self.value.checked_add(&other.value).expect("overflow"),
        }
    }
}

impl<T> AddByRef for CheckedInt<T>
where
    T: CheckedAdd,
{
    fn add_by_ref(&self, other: &Self) -> Self {
        // intentional panic on overflow
        Self {
            value: self.value.checked_add(&other.value).expect("overflow"),
        }
    }
}

impl<T> AddAssign for CheckedInt<T>
where
    T: CheckedAdd,
{
    fn add_assign(&mut self, other: Self) {
        self.value = self.value.checked_add(&other.value).expect("overflow")
    }
}

impl<T> AddAssignByRef for CheckedInt<T>
where
    T: CheckedAdd,
{
    fn add_assign_by_ref(&mut self, other: &Self) {
        self.value = self.value.checked_add(&other.value).expect("overflow")
    }
}

impl<T> MulByRef for CheckedInt<T>
where
    T: CheckedMul,
{
    fn mul_by_ref(&self, rhs: &Self) -> Self {
        // intentional panic on overflow
        Self {
            value: self.value.checked_mul(&rhs.value).expect("overflow"),
        }
    }
}

impl<T> NegByRef for CheckedInt<T>
where
    T: CheckedNeg,
{
    fn neg_by_ref(&self) -> Self {
        Self {
            // intentional panic on overflow
            value: self.value.checked_neg().expect("overflow"),
        }
    }
}

impl<T> Neg for CheckedInt<T>
where
    T: CheckedNeg,
{
    type Output = Self;

    fn neg(self) -> Self {
        Self {
            // intentional panic on overflow
            value: self.value.checked_neg().expect("overflow"),
        }
    }
}

impl<T> HasZero for CheckedInt<T>
where
    T: num::traits::Zero + CheckedAdd,
{
    fn is_zero(&self) -> bool {
        T::is_zero(&self.value)
    }
    fn zero() -> Self {
        CheckedInt::new(T::zero())
    }
}

impl<T> HasOne for CheckedInt<T>
where
    T: num::traits::One + CheckedMul,
{
    fn one() -> Self {
        CheckedInt::new(T::one())
    }
}

impl<T> PartialEq<T> for CheckedInt<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &T) -> bool {
        &self.value == other
    }
}

impl<T> PartialOrd<T> for CheckedInt<T>
where
    T: PartialOrd,
{
    fn partial_cmp(&self, other: &T) -> Option<Ordering> {
        self.value.partial_cmp(other)
    }
}

// Note: this should be generic in T, but the Rust compiler does not like it
// complaining that it conflicts with some implementation in core.
impl<T> From<CheckedInt<T>> for i64
where
    T: Into<i64>,
{
    fn from(value: CheckedInt<T>) -> Self {
        value.value.into()
    }
}

impl<T> From<T> for CheckedInt<T> {
    fn from(value: T) -> Self {
        Self { value }
    }
}

impl<T> Debug for CheckedInt<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        self.value.fmt(f)
    }
}

impl<T> Display for CheckedInt<T>
where
    T: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        self.value.fmt(f)
    }
}

#[cfg(test)]
mod checked_integer_ring_tests {
    use super::*;

    type CheckedI64 = CheckedInt<i64>;

    #[test]
    fn fixed_integer_tests() {
        assert_eq!(0i64, CheckedI64::zero().into());
        assert_eq!(1i64, CheckedI64::one().into());

        let two = CheckedI64::one().add_by_ref(&CheckedI64::one());
        assert_eq!(2i64, two.into());
        assert_eq!(-2i64, two.neg_by_ref().into());
        assert_eq!(-4i64, two.mul_by_ref(&two.neg_by_ref()).into());

        let mut three = two;
        three.add_assign_by_ref(&CheckedI64::from(1i64));
        assert_eq!(3i64, three.into());
        assert!(!three.is_zero());
    }

    #[test]
    #[should_panic]
    fn overflow_test() {
        let max = CheckedI64::from(i64::MAX);
        let _ = max.add_by_ref(&CheckedI64::one());
    }
}
