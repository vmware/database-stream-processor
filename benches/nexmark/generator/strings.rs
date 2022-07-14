//! Generates strings which are used for different field in other model objects.
//!
//! API based on the equivalent [Nexmark Flink StringsGenerator API](https://github.com/nexmark/nexmark/blob/v0.2.0/nexmark-flink/src/main/java/com/github/nexmark/flink/generator/model/StringsGenerator.java).

use super::NexmarkGenerator;
use rand::{distributions::Alphanumeric, Rng};

const MIN_STRING_LENGTH: usize = 3;

/// Return a random string of up to `max_length`.
pub(super) fn next_string<R: Rng>(rng: &mut R, max_length: usize) -> String {
    let len = rng.gen_range(MIN_STRING_LENGTH..=max_length);
    rng.sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

impl<R: Rng> NexmarkGenerator<R> {
    /// Return a random string of up to `max_length`.
    ///
    /// Note: The original java implementation selects from lower-case letters
    /// only adds a special spacer char with a 1 in 13 chance (' ' by default)
    /// If both are necessary, we can update to a less optimized version, but
    /// otherwise it's simpler to use the Alphanumeric distribution.
    pub fn next_string(&mut self, max_length: usize) -> String {
        next_string(&mut self.rng, max_length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::config::Config;
    use rand::rngs::mock::StepRng;

    #[test]
    fn next_string_length() {
        let mut ng = NexmarkGenerator {
            rng: StepRng::new(0, 5),
            config: Config::default(),
        };

        let s = ng.next_string(5);

        assert_eq!(s, "AAA");
    }
}
