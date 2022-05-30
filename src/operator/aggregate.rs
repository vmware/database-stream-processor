//! Aggregation operators.

use std::{borrow::Cow, marker::PhantomData, ops::Neg};

use crate::{
    algebra::{GroupValue, HasOne, ZRingValue, ZSet},
    circuit::{
        operator_traits::{BinaryOperator, Operator, UnaryOperator},
        Circuit, Scope, Stream,
    },
    trace::{cursor::Cursor, BatchReader},
    NumEntries,
};
use deepsize::DeepSizeOf;

impl<P, I> Stream<Circuit<P>, I>
where
    P: Clone + 'static,
    I: Clone + 'static,
{
    // TODO: Consider changing the signature of aggregation function to take a slice
    // of values instead of iterator.  This is easier to understand and use, and
    // allows computing the number of unique values in a group (an important
    // aggregate) in `O(1)`.  Most batch implementations will allow extracting
    // such a slice efficiently.
    /// Aggregate each indexed Z-set in the input stream.
    ///
    /// Values in the input stream are [indexed
    /// Z-sets](`crate::algebra::IndexedZSet`). The aggregation function
    /// `agg_func` takes a single key and the set of (value, weight)
    /// tuples associated with this key and transforms them into a single
    /// aggregate value.  The output of the operator is a Z-set computed as
    /// a sum of aggregates across all keys with weight `+1` each.
    ///
    /// # Type arguments
    ///
    /// * `I` - input indexed Z-set type.
    /// * `O` - output Z-set type.
    pub fn aggregate<F, O>(&self, f: F) -> Stream<Circuit<P>, O>
    where
        I: BatchReader<R = O::R> + 'static,
        F: Fn(&I, &mut I::Cursor) -> O::Key + 'static,
        O: Clone + ZSet + 'static,
        O::R: ZRingValue,
    {
        self.circuit().add_unary_operator(Aggregate::new(f), self)
    }

    /// Incremental version of the [`Aggregate`] operator.
    ///
    /// This is equivalent to `self.integrate().aggregate(f).differentiate()`,
    /// but is more efficient.
    pub fn aggregate_incremental<F, O>(&self, f: F) -> Stream<Circuit<P>, O>
    where
        I: BatchReader<R = O::R> + DeepSizeOf + NumEntries + GroupValue + 'static,
        I::Key: PartialEq,
        F: Fn(&I, &mut I::Cursor) -> O::Key + Clone + 'static,
        O: Clone + ZSet + 'static,
        O::R: ZRingValue,
    {
        let retract_old = self.circuit().add_binary_operator(
            AggregateIncremental::new(false, f.clone()),
            self,
            &self.integrate().delay(),
        );

        let insert_new = self.circuit().add_binary_operator(
            AggregateIncremental::new(true, f),
            self,
            &self.integrate(),
        );

        retract_old.plus(&insert_new)
    }

    /// Incremental nested version of the [`Aggregate`] operator.
    ///
    /// This is equivalent to
    /// `self.integrate().integrate_nested().aggregate(f).differentiate_nested.
    /// differentiate()`, but is more efficient.
    pub fn aggregate_incremental_nested<F, O>(&self, f: F) -> Stream<Circuit<P>, O>
    where
        I: BatchReader<R = O::R> + DeepSizeOf + NumEntries + GroupValue + 'static,
        I::Key: PartialEq,
        F: Fn(&I, &mut I::Cursor) -> O::Key + Clone + 'static,
        O: Clone + ZSet + DeepSizeOf + 'static,
        O::R: ZRingValue,
    {
        self.integrate_nested()
            .aggregate_incremental(f)
            .differentiate_nested()
    }

    /*
    /// A version of [`Self::aggregate_incremental`] optimized for linear
    /// aggregation functions.
    ///
    /// This method only works for linear aggregation functions `f`, i.e.,
    /// functions that satisfy `f(a+b) = f(a) + f(b)`.  It will produce
    /// incorrect results if `f` is not linear.
    ///
    /// Note that this method adds the value of the key from the input indexed
    /// Z-set to the output Z-set, i.e., given an input key-value pair `(k,
    /// v)`, the output Z-set contains value `(k, f(k, v))`.  In contrast,
    /// [`Self::aggregate_incremental`] does not automatically include key
    /// in the output, since a user-defined aggregation function can be
    /// designed to return the key if necessar.  However,
    /// such an aggregation function can be non-linear (in fact, the plus
    /// operation may not even be defined for its output type).
    pub fn aggregate_linear_incremental<F, O>(&self, f: F) -> Stream<Circuit<P>, O>
    where
        <SR as SharedRef>::Target: BatchReader<R=O::R> + DeepSizeOf + NumEntries + GroupValue + SharedRef<Target = SR::Target> + 'static,
        <<SR as SharedRef>::Target as BatchReader>::Key: PartialEq + Clone,
        F: Fn(&<<SR as SharedRef>::Target as BatchReader>::Key,
              &<<SR as SharedRef>::Target as BatchReader>::Val) -> O::Val + 'static,
        O: Clone + ZSet + 'static,
    {
        let agg_delta: Stream<_, OrdZSet<_, _>> = self.map_values(f);
        agg_delta.aggregate_incremental(|zset, cursor| (zset.key().clone(), agg_val.clone()))
    }
    */

    /*
    /// A version of [`Self::aggregate_incremental_nested`] optimized for linear
    /// aggregation functions.
    ///
    /// This method only works for linear aggregation functions `f`, i.e.,
    /// functions that satisfy `f(a+b) = f(a) + f(b)`.  It will produce
    /// incorrect results if `f` is not linear.
    pub fn aggregate_linear_incremental_nested<K, VI, VO, W, F, O>(
        &self,
        f: F,
    ) -> Stream<Circuit<P>, O>
    where
        K: KeyProperties,
        VI: GroupValue,
        SR: SharedRef + 'static,
        <SR as SharedRef>::Target: ZSet<K, VI>,
        <SR as SharedRef>::Target: NumEntries + SharedRef<Target = SR::Target>,
        for<'a> &'a <SR as SharedRef>::Target: IntoIterator<Item = (&'a K, &'a VI)>,
        F: Fn(&K, &VI) -> VO + 'static,
        VO: NumEntries + GroupValue,
        W: ZRingValue,
        O: Clone + MapBuilder<(K, VO), W> + NumEntries + GroupValue,
    {
        self.integrate_nested()
            .aggregate_linear_incremental(f)
            .differentiate_nested()
    }
    */
}

pub struct Aggregate<I, F, O> {
    agg_func: F,
    _type: PhantomData<(I, O)>,
}

impl<I, F, O> Aggregate<I, F, O> {
    pub fn new(agg_func: F) -> Self {
        Self {
            agg_func,
            _type: PhantomData,
        }
    }
}

impl<I, F, O> Operator for Aggregate<I, F, O>
where
    I: 'static,
    F: 'static,
    O: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("Aggregate")
    }
    fn clock_start(&mut self, _scope: Scope) {}
    fn clock_end(&mut self, _scope: Scope) {}
    fn fixedpoint(&self) -> bool {
        true
    }
}

impl<I, F, O> UnaryOperator<I, O> for Aggregate<I, F, O>
where
    I: BatchReader<R = O::R> + 'static,
    F: Fn(&I, &mut I::Cursor) -> O::Key + 'static,
    O: Clone + ZSet + 'static,
    O::R: ZRingValue,
{
    fn eval(&mut self, i: &I) -> O {
        let mut elements = Vec::with_capacity(i.len());
        let mut cursor = i.cursor();

        while cursor.key_valid(i) {
            elements.push((((self.agg_func)(i, &mut cursor), ()), I::R::one()));
            cursor.step_key(i);
        }
        O::from_tuples((), elements)
    }
}

/// Incremental version of the `Aggregate` operator.
///
/// Takes a stream `a` of changes to relation `A` and a stream with delayed
/// value of `A`: `z^-1(A) = a.integrate().delay()` and computes
/// `integrate(A) - integrate(z^-1(A))` incrementally, by only considering
/// values in the support of `a`.
pub struct AggregateIncremental<I, F, O> {
    polarity: bool,
    agg_func: F,
    _type: PhantomData<(I, O)>,
}

impl<I, F, O> AggregateIncremental<I, F, O> {
    pub fn new(polarity: bool, agg_func: F) -> Self {
        Self {
            polarity,
            agg_func,
            _type: PhantomData,
        }
    }
}

impl<I, F, O> Operator for AggregateIncremental<I, F, O>
where
    I: 'static,
    F: 'static,
    O: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("AggregateIncremental")
    }
    fn clock_start(&mut self, _scope: Scope) {}
    fn clock_end(&mut self, _scope: Scope) {}
    fn fixedpoint(&self) -> bool {
        true
    }
}

impl<I, F, O> BinaryOperator<I, I, O> for AggregateIncremental<I, F, O>
where
    I: BatchReader<R = O::R> + 'static,
    I::Key: PartialEq,
    F: Fn(&I, &mut I::Cursor) -> O::Key + 'static,
    O: Clone + ZSet + 'static,
    O::R: ZRingValue,
{
    fn eval(&mut self, delta: &I, integral: &I) -> O {
        let mut result = Vec::with_capacity(delta.len());

        let mut delta_cursor = delta.cursor();
        let mut integral_cursor = integral.cursor();
        let weight = if self.polarity {
            I::R::one()
        } else {
            I::R::one().neg()
        };

        while delta_cursor.key_valid(delta) {
            let key = delta_cursor.key(delta);

            integral_cursor.seek_key(integral, key);

            if integral_cursor.key_valid(integral) && integral_cursor.key(integral) == key {
                // Retract the old value of the aggregate.
                result.push((
                    ((self.agg_func)(integral, &mut integral_cursor), ()),
                    weight.clone(),
                ));
            }
            delta_cursor.step_key(delta);
        }
        O::from_tuples((), result)
    }
}

#[cfg(test)]
mod test {
    use std::{cell::RefCell, rc::Rc};

    use crate::{
        circuit::{Root, Stream},
        operator::{Apply2, GeneratorNested},
        trace::{
            ord::{OrdIndexedZSet, OrdZSet},
            BatchReader, Cursor,
        },
        zset,
    };

    #[test]
    fn aggregate_test() {
        let root = Root::build(move |circuit| {
            let mut inputs = vec![
                vec![
                    zset! { (1, 10) => 1, (1, 20) => 1 },
                    zset! { (2, 10) => 1, (1, 10) => -1, (1, 20) => 1, (3, 10) => 1 },
                ],
                vec![
                    zset! { (4, 20) => 1, (2, 10) => -1 },
                    zset! { (5, 10) => 1, (6, 10) => 1 },
                ],
                vec![],
            ]
            .into_iter();

            circuit
                .iterate(|child| {
                    let counter = Rc::new(RefCell::new(0));
                    let counter_clone = counter.clone();

                    let input: Stream<_, OrdIndexedZSet<usize, usize, isize>> = child
                        .add_source(GeneratorNested::new(Box::new(move || {
                            *counter_clone.borrow_mut() = 0;
                            let mut deltas = inputs.next().unwrap_or_else(Vec::new).into_iter();
                            Box::new(move || deltas.next().unwrap_or_else(|| zset! {}))
                        })))
                        .index();

                    // Weighted sum aggregate.  Returns `(key, weighted_sum)`.
                    let sum = |storage: &OrdIndexedZSet<usize, usize, isize>,
                               cursor: &mut <OrdIndexedZSet<_, _, _> as BatchReader>::Cursor|
                     -> (usize, isize) {
                        let mut result: isize = 0;

                        while cursor.val_valid(storage) {
                            let v = cursor.val(storage);
                            let w = cursor.weight(storage);
                            result += (*v as isize) * w;
                            cursor.step_val(storage);
                        }
                        (cursor.key(storage).clone(), result)
                    };

                    // Weighted sum aggregate that returns only the weighted sum
                    // value and is therefore linear.
                    /*let sum_linear = |_key: &usize, zset: &OrdZSet<usize, isize>| -> isize {
                        let mut result: isize = 0;
                        for (v, w) in zset.into_iter() {
                            result += (*v as isize) * w;
                        }

                        result
                    };*/

                    let sum_inc = input.aggregate_incremental_nested(sum);
                    //let sum_inc_linear = input.aggregate_linear_incremental_nested(sum_linear);
                    let sum_noninc = input
                        .integrate_nested()
                        .integrate()
                        .aggregate(sum)
                        .differentiate()
                        .differentiate_nested();

                    // Compare outputs of all three implementations.
                    child
                        .add_binary_operator(
                            Apply2::new(
                                |d1: &OrdZSet<(usize, isize), isize>,
                                 d2: &OrdZSet<(usize, isize), isize>| {
                                    (d1.clone(), d2.clone())
                                },
                            ),
                            &sum_inc,
                            &sum_noninc,
                        )
                        .inspect(|(d1, d2)| {
                            //println!("incremental: {:?}", d1);
                            //println!("non-incremental: {:?}", d2);
                            assert_eq!(d1, d2);
                        });

                    /*child
                    .add_binary_operator(
                        Apply2::new(
                            |d1: &OrdZSet<(usize, isize), isize>,
                             d2: &OrdZSet<(usize, isize), isize>| {
                                (d1.clone(), d2.clone())
                            },
                        ),
                        &sum_inc,
                        &sum_inc_linear,
                    )
                    .inspect(|(d1, d2)| {
                        assert_eq!(d1, d2);
                    });*/

                    // Min aggregate (non-linear).
                    let min = |storage: &OrdIndexedZSet<usize, usize, isize>,
                               cursor: &mut <OrdIndexedZSet<_, _, _> as BatchReader>::Cursor|
                     -> (usize, usize) {
                        let mut result = usize::MAX;

                        while cursor.val_valid(storage) {
                            let v = cursor.key(storage);
                            if v < &result {
                                result = v.clone();
                            }
                            cursor.step_val(storage);
                        }

                        (cursor.key(storage).clone(), result)
                    };

                    let min_inc = input.aggregate_incremental_nested(min);
                    let min_noninc = input
                        .integrate_nested()
                        .integrate()
                        .aggregate(min)
                        .differentiate()
                        .differentiate_nested();

                    child
                        .add_binary_operator(
                            Apply2::new(
                                |d1: &OrdZSet<(usize, usize), isize>,
                                 d2: &OrdZSet<(usize, usize), isize>| {
                                    (d1.clone(), d2.clone())
                                },
                            ),
                            &min_inc,
                            &min_noninc,
                        )
                        .inspect(|(d1, d2)| {
                            assert_eq!(d1, d2);
                        });

                    Ok((
                        move || {
                            *counter.borrow_mut() += 1;
                            *counter.borrow() == 4
                        },
                        (),
                    ))
                })
                .unwrap();
        })
        .unwrap();

        for _ in 0..3 {
            root.step().unwrap();
        }
    }
}