//! Resting place for [KeenRetryExecutor]


use std::time::Duration;
use crate::{RetryConsumerResult, RetryResult};
use crate::resolved_result::ResolvedResult;

/// Executes the retry logic according to the chosen backoff algorithm and limits, keeping track of retry metrics;
pub enum KeenRetryExecutor<ReportedInput,
                           OriginalInput,
                           Output,
                           ErrorType,
                           RetryFn: FnMut(OriginalInput) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType>> {

    /// Indicates no retrying is needed as the operation completed on the initial attempt
    Resolved(Result<Output, ErrorType>),

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
     ErrorType,
     RetryFn: FnMut(OriginalInput) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>

KeenRetryExecutor<ReportedInput,
                  OriginalInput,
                  Output,
                  ErrorType,
                  RetryFn> {

    pub fn from_resolved(result: Result<Output, ErrorType>) -> Self {
        Self::Resolved(result)
    }

    pub fn new(input: OriginalInput, retry_operation: RetryFn, first_attempt_retryable_error: ErrorType) -> Self {
        Self::ToRetry {
            input,
            retry_operation,
            retry_errors: vec![first_attempt_retryable_error],
        }
    }

    /// Upgrades this [KeenRetryExecutor] into the final [ResolvedResult], possibly executing the `retry_operation` as many times as
    /// there are elements in `delays`, sleeping for the indicated amount on each attempt
    pub fn with_delays(self, delays: impl Iterator<Item=Duration>) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            KeenRetryExecutor::Resolved(result) => ResolvedResult::from(result),
            KeenRetryExecutor::ToRetry { input: mut retry_payload, mut retry_operation, mut retry_errors } => {
                for delay in delays {
                    std::thread::sleep(delay);
                    let new_retry_result = retry_operation(retry_payload);
                    retry_payload = match new_retry_result {
                        RetryResult::Ok    { reported_input, output } => return ResolvedResult::Recovered { reported_input, output, retry_errors },
                        RetryResult::Retry { input, error }          => { retry_errors.push(error); input },
                        RetryResult::Fatal { input, error }          => return ResolvedResult::Unrecoverable { input: Some(input), retry_errors, fatal_error: error },
                    }
                }
                ResolvedResult::GivenUp { input: retry_payload, retry_errors }
            }
        }
    }

}