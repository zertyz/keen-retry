//! Resting place for [KeenRetryAsyncExecutor].\
//! Keep this in sync with ../keen_retry_executor.rs


use std::fmt::Debug;
use std::future::Future;
use std::time::Duration;
use crate::{RetryConsumerResult, RetryResult};
use crate::resolved_result::ResolvedResult;

/// Executes the retry logic according to the chosen backoff algorithm and limits, keeping track of retry metrics;
pub enum KeenRetryAsyncExecutor<ReportedInput,
                                OriginalInput,
                                Output,
                                ErrorType:    Debug,
                                AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
                                OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>> {

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
        input:                 OriginalInput,
        retry_async_operation: AsyncRetryFn,
        retry_errors:          Vec<ErrorType>,
    },

}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType:    Debug,
     AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
     OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>>

KeenRetryAsyncExecutor<ReportedInput,
                       OriginalInput,
                       Output,
                       ErrorType,
                       AsyncRetryFn,
                       OutputFuture> {

    pub fn new(original_input: OriginalInput, retry_async_operation: AsyncRetryFn, first_attempt_retryable_error: ErrorType) -> Self {
        Self::ToRetry {
            input: original_input,
            retry_async_operation,
            retry_errors: vec![first_attempt_retryable_error],
        }
    }

    pub fn from_ok_result(reported_input: ReportedInput, output: Output) -> Self {
        Self::ResolvedOk { reported_input, output }
    }

    pub fn from_err_result(original_input: OriginalInput, error: ErrorType) -> Self {
        Self::ResolvedErr { original_input, error }
    }

    /// Upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], possibly executing the `retry_operation` as many times as
    /// there are elements in `delays`, sleeping for the indicated amount on each attempt.\
    /// See also [Self::spinning_forever()], [Self::yielding_forever()]
    /// Example:
    /// ```nocompile
    ///     // for an arithmetic progression in the sleeping times:
    ///     .with_delays((100..=1000).step_by(100).map(|millis| Duration::from_millis(millis)))
    ///     .await
    ///     // for a geometric progression of a 1.1 ratio:
    ///     .with_delays((100..=1000).map(|millis| Duration::from_millis(1.1.powi(millis))))
    ///     .await
    pub async fn with_delays(self, delays: impl Iterator<Item=Duration>) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            KeenRetryAsyncExecutor::ResolvedOk  { reported_input, output } => ResolvedResult::from_ok_result(reported_input, output),
            KeenRetryAsyncExecutor::ResolvedErr { original_input, error } => ResolvedResult::from_err_result(original_input, error),
            KeenRetryAsyncExecutor::ToRetry { mut input, mut retry_async_operation, mut retry_errors } => {
                for delay in delays {
                    tokio::time::sleep(delay).await;
                    let new_retry_result = retry_async_operation(input).await;
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

    /// Designed for really fast `retry_operation`s, upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], executing the `retry_operation`
    /// until it either succeeds or fatably fails, relaxing the CPU on each attempt -- but not context-switching nor giving `tokio` a change to run other tasks.\
    /// See also [Self::with_delays()], [Self::yielding_forever()]
    pub async fn spinning_forever(self) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            KeenRetryAsyncExecutor::ResolvedOk  { reported_input, output } => ResolvedResult::from_ok_result(reported_input, output),
            KeenRetryAsyncExecutor::ResolvedErr { original_input, error } => ResolvedResult::from_err_result(original_input, error),
            KeenRetryAsyncExecutor::ToRetry { mut input, mut retry_async_operation, mut retry_errors } => {
                loop {
                    // spin 32x
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    let new_retry_result = retry_async_operation(input).await;
                    input = match new_retry_result {
                        RetryResult::Ok    { reported_input, output } => return ResolvedResult::Recovered { reported_input, output, retry_errors },
                        RetryResult::Retry { input, error }          => input,
                        RetryResult::Fatal { input, error }          => return ResolvedResult::Unrecoverable { input, retry_errors, fatal_error: error },
                    }
                }
            }
        }
    }

    /// Upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], executing the `retry_operation`
    /// until it either succeeds or fatably fails, suggesting that `tokio` should run other tasks in-between.\
    /// See also [Self::with_delays()], [Self::spinning_forever()]
    pub async fn yielding_forever(self) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            KeenRetryAsyncExecutor::ResolvedOk  { reported_input, output } => ResolvedResult::from_ok_result(reported_input, output),
            KeenRetryAsyncExecutor::ResolvedErr { original_input, error } => ResolvedResult::from_err_result(original_input, error),
            KeenRetryAsyncExecutor::ToRetry { mut input, mut retry_async_operation, mut retry_errors } => {
                loop {
                    tokio::task::yield_now().await;
                    let new_retry_result = retry_async_operation(input).await;
                    input = match new_retry_result {
                        RetryResult::Ok    { reported_input, output } => return ResolvedResult::Recovered { reported_input, output, retry_errors },
                        RetryResult::Retry { input, error }          => input,
                        RetryResult::Fatal { input, error }          => return ResolvedResult::Unrecoverable { input, retry_errors, fatal_error: error },
                    }
                }
            }
        }
    }

}