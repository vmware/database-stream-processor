use crate::{
    algebra::ZRingValue,
    circuit::{
        operator_traits::{Operator, TernaryOperator},
        Scope,
    },
    operator::trace::{TraceBounds, TraceFeedback},
    trace::{
        cursor::{CursorEmpty, CursorGroup, CursorPair},
        Builder, Cursor, Spine, Trace,
    },
    Circuit, DBData, DBWeight, IndexedZSet, OrdIndexedZSet, RootCircuit, Stream,
};
use std::{borrow::Cow, marker::PhantomData, ops::Neg};

mod topk;
mod lag;

#[cfg(test)]
mod test;

#[derive(PartialEq, Eq)]
pub enum Monotonicity {
    Ascending,
    Descending,
    Unordered,
}

pub trait GroupTransformer<I, O, R>: 'static {
    fn name(&self) -> &str;

    fn monotonicity(&self) -> Monotonicity;

    fn transform<C1, C2, C3, CB>(
        &self,
        input_delta: &mut C1,
        input_trace: &mut C2,
        output_trace: &mut C3,
        output_cb: CB,
    ) where
        C1: Cursor<I, (), (), R>,
        C2: Cursor<I, (), (), R>,
        C3: Cursor<O, (), (), R>,
        CB: FnMut(O, R);
}

pub trait NonIncrementalGroupTransformer<I, O, R>: 'static {
    fn name(&self) -> &str;

    fn monotonicity(&self) -> Monotonicity;

    fn transform<C, CB>(&self, cursor: &mut C, output_cb: CB)
    where
        C: Cursor<I, (), (), R>,
        CB: FnMut(O, R);
}

pub struct DiffGroupTransformer<I, O, R, T> {
    transformer: T,
    _phantom: PhantomData<(I, O, R)>,
}

impl<I, O, R, T> GroupTransformer<I, O, R> for DiffGroupTransformer<I, O, R, T>
where
    I: DBData,
    O: DBData,
    R: DBWeight + Neg<Output = R>,
    T: NonIncrementalGroupTransformer<I, O, R>,
{
    fn name(&self) -> &str {
        self.transformer.name()
    }

    fn monotonicity(&self) -> Monotonicity {
        self.transformer.monotonicity()
    }

    fn transform<C1, C2, C3, CB>(
        &self,
        input_delta: &mut C1,
        input_trace: &mut C2,
        output_trace: &mut C3,
        mut output_cb: CB,
    ) where
        C1: Cursor<I, (), (), R>,
        C2: Cursor<I, (), (), R>,
        C3: Cursor<O, (), (), R>,
        CB: FnMut(O, R),
    {
        match self.transformer.monotonicity() {
            Monotonicity::Ascending => {
                self.transformer.transform(
                    &mut CursorPair::new(input_delta, input_trace),
                    |v, w| {
                        while output_trace.key_valid() && output_trace.key() <= &v {
                            output_cb(output_trace.key().clone(), output_trace.weight().neg());
                            output_trace.step_key();
                        }
                        output_cb(v, w);
                    },
                );

                // Output remaining retractions in output trace.
                while output_trace.key_valid() {
                    output_cb(output_trace.key().clone(), output_trace.weight().neg());
                    output_trace.step_key();
                }
            }

            Monotonicity::Descending => {
                output_trace.fast_forward_keys();
                self.transformer.transform(
                    &mut CursorPair::new(input_delta, input_trace),
                    |v, w| {
                        while output_trace.key_valid() && output_trace.key() >= &v {
                            output_cb(output_trace.key().clone(), output_trace.weight().neg());
                            output_trace.step_key_reverse();
                        }
                        output_cb(v, w);
                    },
                );

                // Output remaining retractions in output trace.
                while output_trace.key_valid() {
                    output_cb(output_trace.key().clone(), output_trace.weight().neg());
                    output_trace.step_key_reverse();
                }
            }

            Monotonicity::Unordered => {
                self.transformer
                    .transform(&mut CursorPair::new(input_delta, input_trace), |v, w| {
                        output_cb(v, w)
                    });

                // Output retractions in output trace.
                while output_trace.key_valid() {
                    output_cb(output_trace.key().clone(), output_trace.weight().neg());
                    output_trace.step_key();
                }
            }
        }
    }
}

impl<I, O, R, T> DiffGroupTransformer<I, O, R, T> {
    fn new(transformer: T) -> Self {
        Self {
            transformer,
            _phantom: PhantomData,
        }
    }
}

/*
pub struct LeanDiffGroupTransformer<I, O, R, T> {
    transformer: T,
}

impl GroupTransformer<I, O, R> for LeanDiffGroupTransformer<I, O, R, T>
where
    T: NonIncrementalGroupTransformer<I, O, R>
{
}

impl<I, O, R, T> LeanDiffGroupTransformer<I, O, R, T> {
    fn new(transformer: T) -> Self {
        Self {
            transformer
        }
    }
}
*/

impl<B> Stream<RootCircuit, B>
where
    B: IndexedZSet + Send,
{
    fn group_transform<GT, OV>(
        &self,
        transform: GT,
    ) -> Stream<RootCircuit, OrdIndexedZSet<B::Key, OV, B::R>>
    where
        GT: GroupTransformer<B::Val, OV, B::R>,
        OV: DBData,
        B::R: ZRingValue,
    {
        self.group_transform_generic(transform)
    }

    fn group_transform_generic<GT, OB>(&self, transform: GT) -> Stream<RootCircuit, OB>
    where
        OB: IndexedZSet<Key = B::Key, R = B::R>,
        OB::Item: Ord,
        GT: GroupTransformer<B::Val, OB::Val, B::R>,
    {
        let circuit = self.circuit();
        let stream = self.shard();

        let bounds = TraceBounds::unbounded();
        let feedback = circuit.add_integrate_trace_feedback::<Spine<OB>>(bounds);

        let output = circuit
            .add_ternary_operator(
                GroupTransform::new(transform),
                &stream,
                &stream.integrate_trace().delay_trace(),
                &feedback.delayed_trace,
            )
            .mark_sharded();

        feedback.connect(&output);

        output
    }
}

struct GroupTransform<B, OB, T, OT, GT>
where
    B: IndexedZSet,
    OB: IndexedZSet,
{
    transformer: GT,
    buffer: Vec<(OB::Item, B::R)>,
    _phantom: PhantomData<(B, OB, T, OT)>,
}

impl<B, OB, T, OT, GT> GroupTransform<B, OB, T, OT, GT>
where
    B: IndexedZSet,
    OB: IndexedZSet,
{
    fn new(transformer: GT) -> Self {
        Self {
            transformer,
            buffer: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

impl<B, OB, T, OT, GT> Operator for GroupTransform<B, OB, T, OT, GT>
where
    B: IndexedZSet + 'static,
    OB: IndexedZSet + 'static,
    T: 'static,
    OT: 'static,
    GT: GroupTransformer<B::Val, OB::Val, B::R>,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from(format!("GroupTransform({})", self.transformer.name()))
    }
    fn fixedpoint(&self, _scope: Scope) -> bool {
        true
    }
}

impl<B, OB, T, OT, GT> TernaryOperator<B, T, OT, OB> for GroupTransform<B, OB, T, OT, GT>
where
    B: IndexedZSet,
    T: Trace<Key = B::Key, Val = B::Val, Time = (), R = B::R> + Clone,
    OB: IndexedZSet<Key = B::Key, R = B::R>,
    OB::Item: Ord,
    OT: Trace<Key = B::Key, Val = OB::Val, Time = (), R = B::R> + Clone,
    GT: GroupTransformer<B::Val, OB::Val, B::R>,
{
    fn eval<'a>(
        &mut self,
        delta: Cow<'a, B>,
        input_trace: Cow<'a, T>,
        output_trace: Cow<'a, OT>,
    ) -> OB {
        let mut delta_cursor = delta.cursor();
        let mut input_trace_cursor = input_trace.cursor();
        let mut output_trace_cursor = output_trace.cursor();

        let mut builder = OB::Builder::with_capacity((), delta.len());

        while delta_cursor.key_valid() {
            let key = delta_cursor.key().clone();

            let mut cb_asc =
                |val: OB::Val, w: B::R| builder.push((OB::item_from(key.clone(), val), w));
            let mut cb_desc =
                |val: OB::Val, w: B::R| self.buffer.push((OB::item_from(key.clone(), val), w));

            let cb = if self.transformer.monotonicity() == Monotonicity::Ascending {
                &mut cb_asc as &mut dyn FnMut(OB::Val, B::R)
            } else {
                &mut cb_desc as &mut dyn FnMut(OB::Val, B::R)
            };

            input_trace_cursor.seek_key(&key);

            // I am not able to avoid 4-way code duplication below.  Depending on
            // whether `key` is found in the input and output trace, we must invoke
            // `transformer.transform` with four different combinations of
            // empty/non-empty cursors.  Since the cursors have different types
            // (`CursorEmpty` and `CursorGroup`), we kind bind them to the same
            // variable.
            if input_trace_cursor.key_valid() && input_trace_cursor.key() == &key {
                let mut input_group_cursor = CursorGroup::new(&mut input_trace_cursor, ());

                output_trace_cursor.seek_key(&key);

                if output_trace_cursor.key_valid() && output_trace_cursor.key() == &key {
                    let mut output_group_cursor = CursorGroup::new(&mut output_trace_cursor, ());

                    self.transformer.transform(
                        &mut CursorGroup::new(&mut delta_cursor, ()),
                        &mut input_group_cursor,
                        &mut output_group_cursor,
                        cb,
                    );
                } else {
                    let mut output_group_cursor = CursorEmpty::new();

                    self.transformer.transform(
                        &mut CursorGroup::new(&mut delta_cursor, ()),
                        &mut input_group_cursor,
                        &mut output_group_cursor,
                        cb,
                    );
                };
            } else {
                let mut input_group_cursor = CursorEmpty::new();

                output_trace_cursor.seek_key(&key);

                if output_trace_cursor.key_valid() && output_trace_cursor.key() == &key {
                    let mut output_group_cursor = CursorGroup::new(&mut output_trace_cursor, ());

                    self.transformer.transform(
                        &mut CursorGroup::new(&mut delta_cursor, ()),
                        &mut input_group_cursor,
                        &mut output_group_cursor,
                        cb,
                    );
                } else {
                    let mut output_group_cursor = CursorEmpty::new();

                    self.transformer.transform(
                        &mut CursorGroup::new(&mut delta_cursor, ()),
                        &mut input_group_cursor,
                        &mut output_group_cursor,
                        cb,
                    );
                };
            };
            match self.transformer.monotonicity() {
                Monotonicity::Descending => {
                    for tuple in self.buffer.drain(..).rev() {
                        builder.push(tuple)
                    }
                }
                Monotonicity::Unordered => {
                    self.buffer.sort();
                    for tuple in self.buffer.drain(..) {
                        builder.push(tuple)
                    }
                }
                _ => {}
            }

            delta_cursor.step_key();
        }

        builder.done()
    }
}