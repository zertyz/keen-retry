//! Common code for the keen-retry executors

use std::{ops::RangeInclusive, time::Duration};


pub enum ExponentialJitter<ErrorType> {

    /// Configs for an exponential backoff with random jitter that retries from the start to the
    /// end of the given range, for up to the given `re_attempts`.
    FromBackoffRange {
        backoff_range_millis: RangeInclusive<u32>,
        re_attempts: u8,
        jitter_ratio: f32,
    },

    /// Configs for an exponential backoff with random jitter that retries until either the
    /// number of `re_attempts` or the `timeout` reaches their limits.\
    /// Example: ```nocompile
    /// ExponentialJitter::UpToTimeout {
    ///     initial_backoff_millis: 100.0,
    ///     expoent:                1.6681005372000588,
    ///     re_attempts:            10,
    ///     jitter_ratio:           0.2,
    ///     timeout:                Duration::from_secs(30),
    ///     timeout_error:          YourErrorType::Timeout,
    /// }
    UpToTimeout {
        initial_backoff_millis: u32,
        expoent: f64,
        re_attempts: u8,
        jitter_ratio: f32,
        timeout: Duration,
        timeout_error: ErrorType,
    },
}
impl<ErrorType> Default for ExponentialJitter<ErrorType> {
    fn default() -> Self {
        Self::FromBackoffRange {
            backoff_range_millis: 100..=10000,
            re_attempts:          10,
            jitter_ratio:         0.2,
        }
    }
}

/// Generates an iterator suitable for usage in backoff strategies for operations that recruit external / shared resources -- such as network services.
/// Its elements progress exponentially from the given `range_millis` start range, going from the first to the last element
/// in `re_attempts` steps -- each of which may be added / subtracted by `jitter_ratio` * `backoff_millis`.\
/// Notice that this method calculates the `expoent` from the given parameters.\
/// As a special case, if the range -- which is expressed in milliseconds -- starts with 0, the first element in the
/// geometric progression will be 0 and the rest of the progression will continue as if it had started with 1
/// -- allowing for zero backoff on the first attempt, which might make sense in highly distributed systems with really low fault rates.\
/// See also [exponential_jitter_from_expoent()]
pub fn exponential_jitter_from_range(backoff_range_millis: RangeInclusive<u32>, re_attempts: u8, jitter_ratio: f32) -> impl Iterator<Item=Duration> {
    let backoff_range_millis = *backoff_range_millis.start() as f64 ..= *backoff_range_millis.end() as f64;
    let expoent = if *backoff_range_millis.start() == 0.0 {
                           backoff_range_millis.end().powf(1.0 / (re_attempts as f64 - 1.0))
                       } else {
                           (backoff_range_millis.end()/backoff_range_millis.start()).powf(1.0 / (re_attempts as f64 - 1.0))
                       };
    exponential_jitter_from_expoent(*backoff_range_millis.start() as u32, expoent, re_attempts, jitter_ratio)
}

/// Generates an iterator suitable for usage in backoff strategies for operations that recruit external / shared resources -- such as network services.
/// Its elements progress exponentially from the given `initial_backoff_millis` with the `expoent` ratio applied to each progression, up to
/// `re_attempts` steps -- each of which may be added / subtracted by `jitter_ratio` * `backoff_millis``.\
/// As a special case, if the range -- which is expressed in milliseconds -- starts with 0, the first element in the
/// geometric progression will be 0 and the rest of the progression will continue as if it had started with 1
/// -- allowing for zero backoff on the first attempt, which might make sense in highly distributed systems with really low fault rates.
/// See also [exponential_jitter_from_range()]
pub fn exponential_jitter_from_expoent(initial_backoff_millis: u32, expoent: f64, re_attempts: u8, jitter_ratio: f32) -> impl Iterator<Item=Duration> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let initial_backoff_millis = initial_backoff_millis as f64;
    let jitter_ratio = jitter_ratio as f64;
    let power_addition = initial_backoff_millis.log(expoent).max(0.0);
    (0..re_attempts)
        .map(move |power| expoent.powf(power as f64 + power_addition))
        .map(move |millis| if millis == 1.0 { initial_backoff_millis } else { millis })
        .map(move |millis| millis*rng.gen_range((1.0-jitter_ratio)..=(1.0+jitter_ratio)) )
        .map(|jittered_millis| Duration::from_millis(jittered_millis as u64))
}

/// Unit tests the [commons](super) module
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_jitter() {

        assert_exponential_jitter(100..=15000, 20, 0.0);
        assert_exponential_jitter(100..=15000, 20, 0.1);
        assert_exponential_jitter(0..=15000, 20, 0.0);
        assert_exponential_jitter(0..=15000, 20, 0.1);

        fn assert_exponential_jitter(range_millis: RangeInclusive<u32>, re_attempts: u8, jitter_ratio: f32) {
            let observed_re_attempts = exponential_jitter_from_range(range_millis.clone(), re_attempts, jitter_ratio).count();
            assert_eq!(observed_re_attempts, re_attempts as usize, "Number of elements mismatch");
            let first = exponential_jitter_from_range(range_millis.clone(), re_attempts, jitter_ratio).next().unwrap().as_millis() as f32;
            assert!((first - *range_millis.start() as f32).abs() <= 1.0 + *range_millis.start() as f32 * jitter_ratio, "First element {first} is not within the range {}..{}", *range_millis.start() as f32 * (1.0-jitter_ratio), *range_millis.start() as f32 * (1.0+jitter_ratio));
            let last = exponential_jitter_from_range(range_millis.clone(), re_attempts, jitter_ratio).last().unwrap().as_millis() as f32;
            assert!((last - *range_millis.end() as f32).abs() <= 1.0 + *range_millis.end() as f32 * jitter_ratio, "Last element {last} is not within the range {}..{}", *range_millis.end() as f32 * (1.0-jitter_ratio), *range_millis.end() as f32 * (1.0+jitter_ratio));
        }
    }
}