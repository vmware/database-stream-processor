#[macro_use]
mod map_macro;
#[cfg(test)]
mod tests;

use crate::{
    algebra::{
        Add, AddAssign, AddAssignByRef, AddByRef, GroupValue, HasZero, Neg, NegByRef,
        WithNumEntries,
    },
    shared_ref_self_generic, NumEntries,
};
use hashbrown::{
    hash_map,
    hash_map::{Entry, HashMap, RawEntryMut},
};
use std::{
    fmt::{Debug, Formatter, Result},
    hash::Hash,
    iter::FromIterator,
    mem::swap,
};

/// The properties we expect for a FiniteMap key.
pub trait KeyProperties: 'static + Clone + Eq + Hash {}

impl<T> KeyProperties for T where T: 'static + Clone + Eq + Hash {}

/// A trait to iterate over keys in the support of a finite map.
///
/// We make this a separate trait, so as not to pollute
/// `trait FiniteMap` with the lifetime parameter.
pub trait WithSupport<'a, KeyType>
where
    KeyType: 'a,
{
    type SupportIterator: Iterator<Item = &'a KeyType>;

    fn support(self) -> Self::SupportIterator;
}

/// Interface to build maps element-by-element.
///
/// This interface is functionally equivalent to `FromIterator<Key, Value>`,
/// and can be used in contexts where (1) laziness is not required,
/// and (2) it is simpler to push data into the map rather than have
/// the map pull data from an iterator.
///
/// `MapBuilder` is a separate trait and not part of `FiniteMap`, since
/// some implementers support only the insertion but not the querying side of
/// the map API.  An example is `Vec<(Key, Value)>`, which implements
/// `MapBuilder`, but not `FiniteMap`.
pub trait MapBuilder<Key, Value> {
    /// Create an empty map.
    fn empty() -> Self;

    /// Create an empty map with specified capacity.
    fn with_capacity(capacity: usize) -> Self;

    /// Increase the value associated with `key` by the specified `value`.
    fn increment(&mut self, key: &Key, value: Value);

    /// Increase the value associated with `key` by the specified `value`.
    fn increment_owned(&mut self, key: Key, value: Value);
}

impl<Key, Value> MapBuilder<Key, Value> for Vec<(Key, Value)>
where
    Key: Clone,
{
    fn empty() -> Self {
        vec![]
    }

    fn with_capacity(capacity: usize) -> Self {
        Vec::with_capacity(capacity)
    }

    fn increment(&mut self, key: &Key, value: Value) {
        self.push((key.clone(), value));
    }

    fn increment_owned(&mut self, key: Key, value: Value) {
        self.push((key, value));
    }
}

/// Finite map trait.
///
/// A finite map maps arbitrary values (comparable for equality)
/// to values in a group.  It has finite support: it is non-zero
/// only for a finite number of values.
///
/// `KeyType` - Type of values stored in finite map.
/// `ValueType` - Type of results.
pub trait FiniteMap<Key, Value>:
    GroupValue
    + IntoIterator<Item = (Key, Value)>
    + FromIterator<(Key, Value)>
    + MapBuilder<Key, Value>
    + WithNumEntries
where
    Key: KeyProperties,
{
    /// Find the value associated to the specified key
    fn lookup(&self, key: &Key) -> Value;

    /// Find the value associated to the specified key.
    ///
    /// Returns `None` when `key` is not in the support of `self`.
    fn get_in_support(&self, key: &Key) -> Option<&Value>;

    /// Modify the value associated with `key`.
    fn update<F>(&mut self, key: &Key, f: F)
    where
        F: FnOnce(&mut Value);

    /// Modify the value associated with `key`.
    fn update_owned<F>(&mut self, key: Key, f: F)
    where
        F: FnOnce(&mut Value);

    /// The size of the support: number of elements for which the map does not
    /// return zero.
    fn support_size(&self) -> usize;
}

#[derive(Clone)]
pub struct FiniteHashMap<Key, Value> {
    // Unfortunately I cannot just implement these traits for
    // HashMap since they conflict with some existing traits.
    // We maintain the invariant that the keys (and only these keys)
    // that have non-zero values are in this map.
    pub(super) value: HashMap<Key, Value>,
}

shared_ref_self_generic!(<Key, Value>, FiniteHashMap<Key, Value>);

impl<Key, Value> NumEntries for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue + NumEntries,
{
    fn num_entries(&self) -> usize {
        match Value::const_num_entries() {
            None => {
                let mut res = 0;
                for (_, v) in self.into_iter() {
                    res += v.num_entries();
                }
                res
            }
            Some(n) => n * self.support_size(),
        }
    }
    fn const_num_entries() -> Option<usize> {
        None
    }
}

impl<Key, Value> FiniteHashMap<Key, Value> {
    /// Create a new map
    pub fn new() -> Self {
        Self {
            value: HashMap::default(),
        }
    }

    /// Create an empty [`FiniteHashMap`] with the capacity to hold `size`
    /// elements without reallocating.
    pub fn with_capacity(size: usize) -> Self {
        Self {
            value: HashMap::with_capacity(size),
        }
    }
}

impl<Key, Value> IntoIterator for FiniteHashMap<Key, Value> {
    type Item = (Key, Value);
    type IntoIter = hash_map::IntoIter<Key, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.value.into_iter()
    }
}

impl<'a, Key, Value> IntoIterator for &'a FiniteHashMap<Key, Value> {
    type Item = (&'a Key, &'a Value);
    type IntoIter = hash_map::Iter<'a, Key, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.value.iter()
    }
}

impl<Key, Value> FromIterator<(Key, Value)> for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (Key, Value)>,
    {
        let mut result = Self::new();
        for (k, v) in iter {
            result.increment(&k, v);
        }

        result
    }
}

impl<Key, Value> WithNumEntries for FiniteHashMap<Key, Value> {
    fn num_entries(&self) -> usize {
        self.value.len()
    }
}

impl<Key, Value> MapBuilder<Key, Value> for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn empty() -> Self {
        Self::new()
    }

    fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity(capacity)
    }

    fn increment(&mut self, key: &Key, value: Value) {
        if value.is_zero() {
            return;
        }

        match self.value.raw_entry_mut().from_key(key) {
            RawEntryMut::Vacant(vacant) => {
                vacant.insert(key.clone(), value);
            }

            RawEntryMut::Occupied(mut occupied) => {
                occupied.get_mut().add_assign(value);
                if occupied.get().is_zero() {
                    occupied.remove_entry();
                }
            }
        }
    }

    fn increment_owned(&mut self, key: Key, value: Value) {
        if value.is_zero() {
            return;
        }

        // This has been a known issue since 2015: https://github.com/rust-lang/rust/issues/56167
        // We should use a different implementation or API if one becomes available.
        match self.value.entry(key) {
            Entry::Vacant(vacant) => {
                vacant.insert(value);
            }

            Entry::Occupied(mut occupied) => {
                occupied.get_mut().add_assign(value);
                if occupied.get().is_zero() {
                    occupied.remove_entry();
                }
            }
        }
    }
}

impl<Key, Value> FiniteMap<Key, Value> for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn lookup(&self, key: &Key) -> Value {
        self.value.get(key).cloned().unwrap_or_else(Value::zero)
    }

    fn get_in_support(&self, key: &Key) -> Option<&Value> {
        self.value.get(key)
    }

    fn update<F>(&mut self, key: &Key, f: F)
    where
        F: FnOnce(&mut Value),
    {
        match self.value.raw_entry_mut().from_key(key) {
            RawEntryMut::Occupied(mut oe) => {
                let val = oe.get_mut();
                f(val);
                if val.is_zero() {
                    oe.remove();
                }
            }
            RawEntryMut::Vacant(ve) => {
                let mut val = Value::zero();
                f(&mut val);
                if !val.is_zero() {
                    ve.insert(key.clone(), val);
                }
            }
        }
    }

    fn update_owned<F>(&mut self, key: Key, f: F)
    where
        F: FnOnce(&mut Value),
    {
        match self.value.entry(key) {
            Entry::Occupied(mut oe) => {
                let val = oe.get_mut();
                f(val);
                if val.is_zero() {
                    oe.remove();
                }
            }
            Entry::Vacant(ve) => {
                let mut val = Value::zero();
                f(&mut val);
                ve.insert(val);
            }
        }
    }

    fn support_size(&self) -> usize {
        self.value.len()
    }
}

impl<'a, Key, Value> WithSupport<'a, Key> for &'a FiniteHashMap<Key, Value> {
    type SupportIterator = hash_map::Keys<'a, Key, Value>;

    fn support(self) -> Self::SupportIterator {
        self.value.keys()
    }
}

impl<Key, Value> Default for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Key, Value> Add for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        fn add_inner<Key, Value>(
            mut this: FiniteHashMap<Key, Value>,
            other: FiniteHashMap<Key, Value>,
        ) -> FiniteHashMap<Key, Value>
        where
            Key: KeyProperties,
            Value: GroupValue,
        {
            for (key, value) in other.value {
                match this.value.entry(key) {
                    Entry::Vacant(vacant) => {
                        vacant.insert(value);
                    }

                    Entry::Occupied(mut occupied) => {
                        occupied.get_mut().add_assign(value);
                        if occupied.get().is_zero() {
                            occupied.remove_entry();
                        }
                    }
                }
            }

            this
        }

        if self.support_size() > other.support_size() {
            add_inner(self, other)
        } else {
            add_inner(other, self)
        }
    }
}
impl<Key, Value> AddByRef for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn add_by_ref(&self, other: &Self) -> Self {
        fn add_inner<Key, Value>(
            mut this: FiniteHashMap<Key, Value>,
            other: &FiniteHashMap<Key, Value>,
        ) -> FiniteHashMap<Key, Value>
        where
            Key: KeyProperties,
            Value: GroupValue,
        {
            for (key, value) in &other.value {
                match this.value.raw_entry_mut().from_key(key) {
                    RawEntryMut::Vacant(vacant) => {
                        vacant.insert(key.clone(), value.clone());
                    }

                    RawEntryMut::Occupied(mut occupied) => {
                        occupied.get_mut().add_assign_by_ref(value);
                        if occupied.get().is_zero() {
                            occupied.remove_entry();
                        }
                    }
                }
            }

            this
        }

        if self.support_size() > other.support_size() {
            add_inner(self.clone(), other)
        } else {
            add_inner(other.clone(), self)
        }
    }
}

impl<Key, Value> AddAssign for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn add_assign(&mut self, other: Self) {
        for (key, value) in other.value {
            match self.value.entry(key) {
                Entry::Vacant(vacant) => {
                    vacant.insert(value);
                }

                Entry::Occupied(mut occupied) => {
                    occupied.get_mut().add_assign(value);
                    if occupied.get().is_zero() {
                        occupied.remove_entry();
                    }
                }
            }
        }
    }
}

impl<KeyType, ValueType> AddAssignByRef for FiniteHashMap<KeyType, ValueType>
where
    KeyType: KeyProperties,
    ValueType: GroupValue,
{
    fn add_assign_by_ref(&mut self, other: &Self) {
        for (key, value) in &other.value {
            match self.value.raw_entry_mut().from_key(key) {
                RawEntryMut::Vacant(vacant) => {
                    vacant.insert(key.clone(), value.clone());
                }

                RawEntryMut::Occupied(mut occupied) => {
                    occupied.get_mut().add_assign_by_ref(value);
                    if occupied.get().is_zero() {
                        occupied.remove_entry();
                    }
                }
            }
        }
    }
}

impl<Key, Value> HasZero for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn is_zero(&self) -> bool {
        self.value.is_empty()
    }

    fn zero() -> Self {
        Self::default()
    }
}

impl<Key, Value> NegByRef for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn neg_by_ref(&self) -> Self {
        let mut result = self.clone();
        for val in result.value.values_mut() {
            let mut tmp = Value::zero();
            swap(val, &mut tmp);
            *val = tmp.neg();
        }

        result
    }
}

impl<Key, Value> Neg for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    type Output = Self;

    fn neg(mut self) -> Self {
        for val in self.value.values_mut() {
            let mut tmp = Value::zero();
            swap(val, &mut tmp);
            *val = tmp.neg();
        }

        self
    }
}

impl<Key, Value> PartialEq for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<Key, Value> Eq for FiniteHashMap<Key, Value>
where
    Key: KeyProperties,
    Value: GroupValue,
{
}

impl<K, V> Debug for FiniteHashMap<K, V>
where
    K: Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.value.fmt(f)
    }
}
