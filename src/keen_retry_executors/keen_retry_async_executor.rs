//! Resting place for [KeenRetryAsyncExecutor]


use std::future::Future;
use std::time::Duration;
use crate::{RetryConsumerResult, RetryResult};
use crate::resolved_result::ResolvedResult;

/// Executes the retry logic according to the chosen backoff algorithm and limits, keeping track of retry metrics;
pub enum KeenRetryAsyncExecutor<ReportedInput,
                                OriginalInput,
                                Output,
                                ErrorType,
                                AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
                                OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>> {

    /// Indicates no retrying is needed as the operation completed on the initial attempt
    Resolved(Result<Output, ErrorType>),

    /// Indicates executing retries is, indeed, needed, as the initial operation resulted into a retryable error
    ToRetry {
        retry_payload:         OriginalInput,
        retry_async_operation: AsyncRetryFn,
        retry_errors:          Vec<ErrorType>,
    },

}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType,
     AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
     OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>>

KeenRetryAsyncExecutor<ReportedInput,
                       OriginalInput,
                       Output,
                       ErrorType,
                       AsyncRetryFn,
                       OutputFuture> {

    pub fn from_resolved(result: Result<Output, ErrorType>) -> Self {
        Self::Resolved(result)
    }

    pub fn new(retry_payload: OriginalInput, retry_async_operation: AsyncRetryFn, first_attempt_retryable_error: ErrorType) -> Self {
        Self::ToRetry {
            retry_payload,
            retry_async_operation,
            retry_errors: vec![first_attempt_retryable_error],
        }
    }

    /// Upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], possibly executing the `retry_operation` as many times as
    /// there are elements in `delays`, sleeping for the indicated amount on each attempt
    pub async fn with_delays(self, delays: impl Iterator<Item=Duration>) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            KeenRetryAsyncExecutor::Resolved(result) => ResolvedResult::from(result),
            KeenRetryAsyncExecutor::ToRetry { mut retry_payload, mut retry_async_operation, mut retry_errors } => {
                for delay in delays {
                    tokio::time::sleep(delay).await;
                    let new_retry_result = retry_async_operation(retry_payload).await;
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