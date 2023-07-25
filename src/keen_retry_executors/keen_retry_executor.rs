//! Resting place for [KeenRetryExecutor]


use std::time::Duration;
use crate::RetryConsumerResult;
use crate::resolved_result::ResolvedResult;

/// Executes the retry logic according to the chosen backoff algorithm and limits, keeping track of retry metrics;
pub enum KeenRetryExecutor<OkResult,
                           RetryPayload,
                           ErrorType,
                           RetryFn: FnMut(RetryPayload) -> RetryConsumerResult<OkResult, RetryPayload, ErrorType>> {

    /// Indicates no retrying is needed as the operation completed on the initial attempt
    Resolved(Result<OkResult, ErrorType>),

    /// Indicates executing retries is, indeed, needed, as the initial operation resulted into a retryable error
    ToRetry {
        retry_payload:   RetryPayload,
        retry_operation: RetryFn,
        retry_errors:    Vec<ErrorType>,
    },

}

impl<OkResult,
     RetryPayload,
     ErrorType,
     RetryFn: FnMut(RetryPayload) -> RetryConsumerResult<OkResult, RetryPayload, ErrorType>>

KeenRetryExecutor<OkResult,
                  RetryPayload,
                  ErrorType,
                  RetryFn> {

    pub fn from_resolved(result: Result<OkResult, ErrorType>) -> Self {
        Self::Resolved(result)
    }

    pub fn new(retry_payload: RetryPayload, retry_operation: RetryFn, first_attempt_retryable_error: ErrorType) -> Self {
        Self::ToRetry {
            retry_payload,
            retry_operation,
            retry_errors: vec![first_attempt_retryable_error],
        }
    }

    /// Upgrades this [KeenRetryExecutor] into the final [ResolvedResult], possibly executing the `retry_operation` as many times as
    /// there are elements in `delays`, sleeping for the indicated amount on each attempt
    pub fn with_delays(self, delays: impl Iterator<Item=Duration>) -> ResolvedResult<OkResult, RetryPayload, ErrorType> {
        match self {
            KeenRetryExecutor::Resolved(result) => ResolvedResult::from(result),
            KeenRetryExecutor::ToRetry { mut retry_payload, mut retry_operation, mut retry_errors } => {
                for delay in delays {
                    std::thread::sleep(delay);
                    let new_retry_result = retry_operation(retry_payload);
                    retry_payload = match new_retry_result {
                        RetryConsumerResult::Ok    { payload }                    => return ResolvedResult::Recovered { payload, retry_errors },
                        RetryConsumerResult::Retry { payload, error } => { retry_errors.push(error); payload },
                        RetryConsumerResult::Fatal { payload, error } => return ResolvedResult::Unrecoverable { payload: Some(payload), retry_errors, fatal_error: error },
                    }
                }
                ResolvedResult::GivenUp { payload: retry_payload, retry_errors }
            }
        }
    }

}