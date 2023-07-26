//! Resting place for [KeenRetryExecutor].\
// //! Keep this in sync with ../keen_retry_async_executor.rs


use std::fmt::Debug;
use std::time::Duration;
use crate::{RetryConsumerResult, RetryResult};
use crate::resolved_result::ResolvedResult;

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
    pub fn with_delays(self, delays: impl Iterator<Item=Duration>) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            KeenRetryExecutor::ResolvedOk  { reported_input, output } => ResolvedResult::from_ok_result(reported_input, output),
            KeenRetryExecutor::ResolvedErr { original_input, error } => ResolvedResult::from_err_result(original_input, error),
            KeenRetryExecutor::ToRetry { mut input, mut retry_operation, mut retry_errors } => {
                for delay in delays {
                    std::thread::sleep(delay);
                    let new_retry_result = retry_operation(input);
                    input = match new_retry_result {
                        RetryResult::Ok    { reported_input, output } => return ResolvedResult::Recovered { reported_input, output, retry_errors },
                        RetryResult::Retry { input, error }          => { retry_errors.push(error); input },
                        RetryResult::Fatal { input, error }          => return ResolvedResult::Unrecoverable { input, retry_errors, fatal_error: error },
                    }
                }
                // retries exhausted without success: report as `GivenUp` (unless the number of retries was 0)
                let fatal_error = retry_errors.pop();
                match fatal_error {
                    Some(fatal_error) => ResolvedResult::GivenUp       { input, retry_errors, fatal_error },
                    None                        => panic!("BUG! the `keen-retry` crate has a bug in the way it start retries: a retry can only be created with the first error having been registered"),
                }
            }
        }
    }

}