//! Common code for the keen-retry executors

use std::{ops::RangeInclusive, time::Duration};


/// Configuration options for the "Exponential with Random Jitter" backoff strategy
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
    ///     exponent:                1.6681005372000588,
    ///     re_attempts:            10,
    ///     jitter_ratio:           0.2,
    ///     timeout:                Duration::from_secs(30),
    ///     timeout_error:          YourErrorType::Timeout,
    /// }
    UpToTimeout {
        initial_backoff_millis: u32,
        exponent: f64,
        re_attempts: u8,
        jitter_ratio: f32,
        timeout: Duration,
        timeout_error: ErrorType,
    },
}
impl<ErrorType> Default for ExponentialJitter<ErrorType> {

    #[inline(always)]
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
/// Notice that this method calculates the `exponent` from the given parameters.\
/// As a special case, if the range -- which is expressed in milliseconds -- starts with 0, the first element in the
/// geometric progression will be 0 and the rest of the progression will continue as if it had started with 1
/// -- allowing for zero backoff on the first attempt, which might make sense in highly distributed systems with really low fault rates.\
/// See also [exponential_jitter_from_exponent()]
#[inline(always)]
pub fn exponential_jitter_from_range(backoff_range_millis: RangeInclusive<u32>, re_attempts: u8, jitter_ratio: f32) -> impl Iterator<Item=Duration> {
    let backoff_range_millis = *backoff_range_millis.start() as f64 ..= *backoff_range_millis.end() as f64;
    let exponent = if *backoff_range_millis.start() == 0.0 {
                           backoff_range_millis.end().powf(1.0 / (re_attempts as f64 - 1.0))
                       } else {
                           (backoff_range_millis.end()/backoff_range_millis.start()).powf(1.0 / (re_attempts as f64 - 1.0))
                       };
    exponential_jitter_from_exponent(*backoff_range_millis.start() as u32, exponent, re_attempts, jitter_ratio)
}

/// Generates an iterator suitable for usage in backoff strategies for operations that recruit external / shared resources -- such as network services.
/// Its elements progress exponentially from the given `initial_backoff_millis` with the `exponent` ratio applied to each progression, up to
/// `re_attempts` steps -- each of which may be added / subtracted by `jitter_ratio` * `backoff_millis``.\
/// As a special case, if the `initial_backoff_millis` starts with 0, the first element in the
/// geometric progression will be 0 and the rest of the progression will continue as if it had started with 1
/// -- allowing for zero backoff on the first attempt, which might make sense in highly distributed systems with really low fault rates.
/// See also [exponential_jitter_from_range()]
#[inline(always)]
pub fn exponential_jitter_from_exponent(initial_backoff_millis: u32, exponent: f64, mut re_attempts: u8, jitter_ratio: f32) -> impl Iterator<Item=Duration> {
    use rand::{Rng, SeedableRng};
    let mut rnd = rand::rngs::SmallRng::seed_from_u64(( (((1.0 + jitter_ratio as f64) * (1.0 + exponent) * (re_attempts as f64 * 65536.0)) as u64) << 32 ) + initial_backoff_millis as u64);
    let initial_backoff_millis = initial_backoff_millis as f64;
    let jitter_ratio = jitter_ratio as f64;
    let power_addition = initial_backoff_millis.log(exponent).max(-initial_backoff_millis);
    // protect against 0 delay between re-attempts
    if (power_addition < 0.0 || power_addition.is_infinite()) && initial_backoff_millis == 0.0 {
        re_attempts = 0;
    } else if exponent == 0.0 {
        re_attempts = if initial_backoff_millis > 0.0 { 1 } else { 0 };
    }
    (0..re_attempts)
        .map(move |power| exponent.powf(power as f64 + power_addition))
        .map(move |millis| if millis == 1.0 { initial_backoff_millis } else { millis })
        .map(move |millis| millis*rnd.random_range((1.0-jitter_ratio)..=(1.0+jitter_ratio)) )
        .map(|jittered_millis| Duration::from_millis(jittered_millis as u64))
}

/// Unit tests the [commons](super) module
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_jitter() {

        // // debug
        // for (i, e) in exponential_jitter_from_exponent(100, 0.0, 20, 0.0).enumerate() {
        //     println!("ELEMENT #{i}: {e:?}");
        // }

        // happy path
        /////////////

        assert_exponential_jitter(100..=15000, 20, 0.0);
        assert_exponential_jitter(100..=15000, 20, 0.1);
        assert_exponential_jitter(0..=15000, 20, 0.0);
        assert_exponential_jitter(0..=15000, 20, 0.1);

        // edge cases
        /////////////

        assert_exponential_jitter(0..=1, 1, 0.0);
        assert_eq!(exponential_jitter_from_range(0..=0, 1, 0.0).next(), None, "ZERO range should mean no elements!");
        assert_eq!(exponential_jitter_from_range(0..=1, 0, 0.0).next(), None, "ZERO re-attempts should mean no elements!");
        assert_eq!(exponential_jitter_from_range(0..=0, 0, 0.0).next(), None, "ZERO re-attempts should mean no elements!");
        assert_eq!(exponential_jitter_from_exponent(100, 0.0, 20, 0.0).count(), 1, "ZERO exponent should yield a single element when initial backoff > 0");
        assert_eq!(exponential_jitter_from_exponent(0, 0.0, 20, 0.0).count(), 0, "ZERO exponent should yield no elements when initial backoff == 0");

        // negative progression
        ///////////////////////
        assert_eq!(sum_exponential(100, 0.9, 20), 870, "Negative progression no longer works -- we expect this sequence to start in 100 and have each element decreased by 10%");
        assert_eq!(exponential_jitter_from_exponent(0, 0.9, 20, 0.0).count(), 0, "Negative progression from negative exponent should amount to ZERO elements -- no retrying at all (as we should not allow 0 delay between exponential progressions)");

        fn assert_exponential_jitter(range_millis: RangeInclusive<u32>, re_attempts: u8, jitter_ratio: f32) {
            let observed_re_attempts = exponential_jitter_from_range(range_millis.clone(), re_attempts, jitter_ratio).count();
            assert_eq!(observed_re_attempts, re_attempts as usize, "Number of elements mismatch");
            let first = exponential_jitter_from_range(range_millis.clone(), re_attempts, jitter_ratio).next().unwrap().as_millis() as f32;
            assert!((first - *range_millis.start() as f32).abs() <= 1.0 + *range_millis.start() as f32 * jitter_ratio, "First element {first} is not within the range {}..{}", *range_millis.start() as f32 * (1.0-jitter_ratio), *range_millis.start() as f32 * (1.0+jitter_ratio));
            let last = exponential_jitter_from_range(range_millis.clone(), re_attempts, jitter_ratio).last().unwrap().as_millis() as f32;
            assert!((last - *range_millis.end() as f32).abs() <= 1.0 + *range_millis.end() as f32 * jitter_ratio, "Last element {last} is not within the range {}..{}", *range_millis.end() as f32 * (1.0-jitter_ratio), *range_millis.end() as f32 * (1.0+jitter_ratio));
        }

        fn sum_exponential(initial_backoff_millis: u32, exponent: f64, re_attempts: u8) -> u128 {
            exponential_jitter_from_exponent(initial_backoff_millis, exponent, re_attempts, 0.0)
                .map(|duration| duration.as_millis())
                .sum()
        }
    }
}