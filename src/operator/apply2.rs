//! Binary operator that applies an arbitrary binary function to its inputs.

use crate::circuit::{
    operator_traits::{BinaryOperator, Operator},
    Circuit, Scope, Stream,
};
use std::borrow::Cow;

impl<P, T1> Stream<Circuit<P>, T1>
where
    P: Clone + 'static,
    T1: Clone + 'static,
{
    /// Apply a user-provided binary function to its inputs at each timestamp.
    pub fn apply2<F, T2, T3>(
        &self,
        other: &Stream<Circuit<P>, T2>,
        func: F,
    ) -> Stream<Circuit<P>, T3>
    where
        T2: Clone + 'static,
        T3: Clone + 'static,
        F: Fn(&T1, &T2) -> T3 + 'static,
    {
        self.circuit()
            .add_binary_operator(Apply2::new(func), self, other)
    }
}

/// Applies a user-provided binary function to its inputs at each timestamp.
pub struct Apply2<F> {
    func: F,
}

impl<F> Apply2<F> {
    pub const fn new(func: F) -> Self
    where
        F: 'static,
    {
        Self { func }
    }
}

impl<F> Operator for Apply2<F>
where
    F: 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::from("Apply2")
    }

    fn fixedpoint(&self, _scope: Scope) -> bool {
        // TODO: either change `F` type to `Fn` from `FnMut` or
        // parameterize the operator with custom fixed point check.
        unimplemented!();
    }
}

impl<T1, T2, T3, F> BinaryOperator<T1, T2, T3> for Apply2<F>
where
    F: Fn(&T1, &T2) -> T3 + 'static,
{
    fn eval(&mut self, i1: &T1, i2: &T2) -> T3 {
        (self.func)(i1, i2)
    }
}

#[cfg(test)]
mod test {
    use crate::{circuit::Root, operator::Generator};
    use std::vec;

    #[test]
    fn apply2_test() {
        let root = Root::build(move |circuit| {
            let mut inputs1 = vec![1, 2, 3].into_iter();
            let mut inputs2 = vec![-1, -2, -3].into_iter();

            let source1 = circuit.add_source(Generator::new(move || inputs1.next().unwrap()));
            let source2 = circuit.add_source(Generator::new(move || inputs2.next().unwrap()));

            source1
                .apply2(&source2, |x, y| *x + *y)
                .inspect(|z| assert_eq!(*z, 0));
        })
        .unwrap();

        for _ in 0..3 {
            root.step().unwrap();
        }
    }
}
