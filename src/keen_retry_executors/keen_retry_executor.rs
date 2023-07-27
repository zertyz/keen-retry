//! Resting place for [KeenRetryExecutor].\
// //! Keep this in sync with ../keen_retry_async_executor.rs


use crate::{
    resolved_result::ResolvedResult,
    RetryResult,
};
use std::{
    time::{Duration, SystemTime},
    fmt::Debug,
};

/// Executes the retry logic according to the chosen backoff algorithm and limits, keeping track of retry metrics;
pub enum KeenRetryExecutor<ReportedInput,
                           OriginalInput,
                           Output,
                           ErrorType: Debug,
                           RetryFn:   FnMut(OriginalInput) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType>> {


    /// Indicates no retrying is needed as the operation completed successfully on the initial attempt
    ResolvedOk {
        reported_input: ReportedInput,
        output:         Output
    },

    /// Indicates no retrying is needed as the operation completed fatally on the initial attempt
    ResolvedErr {
        original_input: OriginalInput,
        error:          ErrorType
    },

    /// Indicates executing retries is, indeed, needed, as the initial operation resulted into a retryable error
    ToRetry {
        input:           OriginalInput,
        retry_operation: RetryFn,
        retry_errors:    Vec<ErrorType>,
    },

}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType: Debug,
     RetryFn:   FnMut(OriginalInput) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>


KeenRetryExecutor<ReportedInput,
                  OriginalInput,
                  Output,
                  ErrorType,
                  RetryFn> {


    pub fn new(original_input: OriginalInput, retry_operation: RetryFn, first_attempt_retryable_error: ErrorType) -> Self {
        Self::ToRetry {
            input: original_input,
            retry_operation,
            retry_errors: vec![first_attempt_retryable_error],
        }
    }

    pub fn from_ok_result(reported_input: ReportedInput, output: Output) -> Self {
        Self::ResolvedOk { reported_input, output }
    }

    pub fn from_err_result(original_input: OriginalInput, error: ErrorType) -> Self {
        Self::ResolvedErr { original_input, error }
    }

    /// Upgrades this [KeenRetryExecutor] into the final [ResolvedResult], possibly executing the `retry_operation` as many times as
    /// there are elements in `delays`, sleeping for the indicated amount on each attempt
    /// See also [Self::spinning_forever()]
    /// Example:
    /// ```nocompile
    ///     // for an arithmetic progression in the sleeping times:
    ///     .with_delays((100..=1000).step_by(100).map(|millis| Duration::from_millis(millis)))
    ///     // for a geometric progression with a 1.289 ratio in 13 steps: sleeps from 1 to ~350ms
    ///     .with_delays((1..=13).map(|millis| Duration::from_millis((millis as f64 * 1.289f64.powi(millis)) as u64)))
    pub fn with_delays(self, mut delays: impl Iterator<Item=Duration>) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        self.retry_loop(
            move |input, mut retry_errors| {
                match delays.next() {
                    Some(delay) => {
                        std::thread::sleep(delay);
                        Ok((input, retry_errors))
                    },
                    None => {
                        // retries exhausted without success: report as `GivenUp` (unless the number of retries was 0)
                        let fatal_error = retry_errors.pop();
                        match fatal_error {
                            Some(fatal_error) => Err(ResolvedResult::GivenUp { input, retry_errors, fatal_error }),
                            None                        => panic!("BUG! the `keen-retry` crate has a bug in the way it start retries: a retry can only be created with the first retryable error having been registered"),
                        }
                    },
                }
            },
            |_input, error, retry_errors_list| retry_errors_list.push(error)
        )
    }

    /// Designed for really fast `retry_operation`s, upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], executing the `retry_operation`
    /// until it either succeeds or fatably fails, relaxing the CPU on each attempt -- but not context-switching nor giving `tokio` a change to run other tasks.\
    /// See also [Self::with_delays()]
    pub fn spinning_forever(self) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        self.retry_loop(
            |input, retry_errors| {
                // spin 32x
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                Ok((input, retry_errors))
            },
            |_input, _error, _retry_errors_list| ()
        )
    }

    /// Upgrades this [KeenRetryExecutor] into the final [ResolvedResult], executing the `retry_operation`
    /// until it either succeeds (or fatably fails) or `timeout` elapses -- in which case, the operation will fail
    /// with the error given by `timeout_error`.\
    /// Upon each new attempt, some time will be spent in the CPU Relaxed state & a context switch is likely to happen (to fetch the time)
    /// -- since the CPU usage will still be 100%, using this method is recommended only for very short durations (<~100ms).
    pub fn spinning_until_timeout(self,
                                  timeout:       Duration,
                                  timeout_error: ErrorType)
                                 -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        let mut timeout_error = Some(timeout_error);
        let start = SystemTime::now();
        self.retry_loop(
            |input, retry_errors| {
                // spin 32x
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                if start.elapsed().expect("keen_retry: error determining the elapsed time") >= timeout {
                    Err(ResolvedResult::GivenUp { input, retry_errors, fatal_error: timeout_error.take().unwrap() })
                } else {
                    Ok((input, retry_errors))
                }
            },
            |_input, _error, _retry_errors_list| ()
        )
    }


    /// The low-level generic retry loop for this async executor -- for when [Self::with_delays()], [Self::spinning_forever()]
    /// nor [Self::spinning_until_timeout()] suit your needs:
    ///   - `on_pre_reattempt(&input, &mut retry_errors_list) -> Result<(), ErrorType>`
    ///     Called before performing (another) retry attempt. If it returns a non-`Ok` value, the returned error
    ///     will be reported as `fatal` and the `retry_loop()` will end.
    ///   - `on_non_fatal_failure(&input, error, &mut retry_errors_list)`
    ///     Called when the current retry attempt failed. Used to, optionally, push the error in the `retry_errors_list`.
    /// See the sources of [Self::with_delays()] for a good example of how to use this low level function.
    pub fn retry_loop(self,
                      mut on_pre_attempt:       impl FnMut(OriginalInput,  Vec<ErrorType>) -> Result<(OriginalInput, Vec<ErrorType>), ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType>>,
                      mut on_non_fatal_failure: impl FnMut(&OriginalInput, ErrorType, &mut Vec<ErrorType>))

                     -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {

        match self {
            KeenRetryExecutor::ResolvedOk  { reported_input, output } => ResolvedResult::from_ok_result(reported_input, output),
            KeenRetryExecutor::ResolvedErr { original_input, error } => ResolvedResult::from_err_result(original_input, error),
            KeenRetryExecutor::ToRetry { mut input, mut retry_operation, mut retry_errors } => {
                loop {
                    (input, retry_errors) = match on_pre_attempt(input, retry_errors) {
                        Ok((input, mut retry_errors)) => {
                            let new_retry_result = retry_operation(input);
                            match new_retry_result {
                                RetryResult::Ok    { reported_input, output } => break ResolvedResult::Recovered { reported_input, output, retry_errors },
                                RetryResult::Fatal { input, error }          => break ResolvedResult::Unrecoverable { input, retry_errors, fatal_error: error },
                                RetryResult::Retry { input, error }          => {
                                    on_non_fatal_failure(&input, error, &mut retry_errors);
                                    (input, retry_errors)
                                },
                            }
                        },
                        Err(resolved_result) => {
                            break resolved_result
                        }
                    };
                }
            }
        }
    }
}