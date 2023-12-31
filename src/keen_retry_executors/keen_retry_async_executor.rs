//! Resting place for [KeenRetryAsyncExecutor].\
//! Keep this in sync with ../keen_retry_executor.rs


use crate::{
    keen_retry_executors::common,
    resolved_result::ResolvedResult,
    RetryResult, ExponentialJitter,
};
use std::{
    time::{Duration, SystemTime},
    future::{Future, self},
};

/// Executes the retry logic according to the chosen backoff algorithm and limits, keeping track of retry metrics;
pub enum KeenRetryAsyncExecutor<ReportedInput,
                                OriginalInput,
                                Output,
                                ErrorType,
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
     ErrorType,
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

    /// The recommended backoff strategy when retrying operations that consume external / shared resources -- such as network services.
    /// This strategy delays each attempt by a growing duration + a random component, so to avoid the "thundering herd problem".
    /// Moreover, it allows the first retry to be done immediately, if the `range_millis` starts with zero -- this assumes any failures
    /// are rare and may be handled immediately by another node upon retrying. If this doesn't hold true, further re-attempts will be delayed.\
    /// Calling this method upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult].\
    /// See also:
    ///   * [Self::yielding_forever()] or [Self::yielding_until_timeout()] for retrying local operations;
    ///   * [Self::with_delays()] for custom backoffs.
    pub async fn with_exponential_jitter(self,
                                         config_fn: impl FnOnce() -> ExponentialJitter<ErrorType>)
                                         -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match config_fn() {
        
            ExponentialJitter::FromBackoffRange {
                backoff_range_millis,
                re_attempts,
                jitter_ratio,
            } => self.with_delays(common::exponential_jitter_from_range(backoff_range_millis, re_attempts, jitter_ratio)).await,

            ExponentialJitter::UpToTimeout {
                initial_backoff_millis,
                expoent,
                re_attempts,
                jitter_ratio,
                timeout,
                timeout_error,
             } => self.with_delays_and_timeout(common::exponential_jitter_from_expoent(initial_backoff_millis, expoent, re_attempts, jitter_ratio), timeout, Some(timeout_error)).await,
        }

    }
    
    /// Upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], possibly executing the `retry_operation` as many times as
    /// there are elements in `delays`, sleeping for the indicated amount on each attempt.\
    /// See also [Self::with_exponential_jitter()], [Self::spinning_forever()], [Self::yielding_forever()]
    /// Example:
    /// ```nocompile
    ///     // for an arithmetic progression in the sleeping times:
    ///     .with_delays((100..=1000).step_by(100).map(|millis| Duration::from_millis(millis)))
    ///     .await
    ///     // for a geometric progression with a 1.289 ratio in 13 steps: sleeps from 1 to ~350ms
    ///     .with_delays((1..=13).map(|millis| Duration::from_millis(1.289f64.powi(millis)) as u64))
    ///     .await
    pub async fn with_delays(self, delays: impl Iterator<Item=Duration>) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        self.with_delays_and_timeout(delays, Duration::ZERO, None).await
    }

    /// Similar to [Self::with_delays()], but enforces a timeout for the whole retrying process -- useful for nested retry operations, which may quickly scale up.\
    /// If `timeout_error` is `None`, no timeout is enforced and this method behaves exactly like [Self::with_delays()]
    pub async fn with_delays_and_timeout(self,
                                         mut delays:        impl Iterator<Item=Duration>,
                                         timeout:           Duration,
                                         mut timeout_error: Option<ErrorType>)
                                        -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        let start = if timeout_error.is_none() { SystemTime::UNIX_EPOCH } else { SystemTime::now() };
        self.retry_loop(
            move |input, mut retry_errors| {
                let next_delay = delays.next();
                let mut timeout_error = timeout_error.take();
                async move {
                    match next_delay {
                        Some(delay) => {
                            if timeout_error.is_some() && start.elapsed().unwrap_or_default() >= timeout - delay {
                                Err(ResolvedResult::GivenUp { input, retry_errors, fatal_error: timeout_error.take().unwrap() })
                            } else {
                                if delay > Duration::ZERO {
                                    tokio::time::sleep(delay).await;
                                }
                                Ok((input, retry_errors))
                            }
                        },
                        None => {
                            // retries exhausted without success: report as `GivenUp` (unless the number of retries was 0)
                            let fatal_error = retry_errors.pop();
                            match fatal_error {
                                Some(fatal_error) => Err(ResolvedResult::GivenUp { input, retry_errors, fatal_error }),
                                None                         => panic!("BUG! the `keen-retry` crate has a bug in the way it start retries: a retry can only be created with the first retryable error having been registered"),
                            }
                        },
                    }
                }
            },
            |_input, error, retry_errors_list| {
                retry_errors_list.push(error);
                future::ready(())
            }
        ).await
    }


    /// Designed for really fast `retry_operation`s, providing the lowest possible latency, upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult],
    /// executing the `retry_operation` until it either succeeds or fatably fails, relaxing the CPU on each attempt -- but not context-switching nor giving `tokio`
    /// a chance to run other tasks.\
    /// Use with caution, as this method may dead-lock the thread, at 100% CPU usage, as there is no limit for the number of retries.\
    /// See also [Self::with_delays()], [Self::yielding_forever()]
    pub async fn spinning_forever(self) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        self.retry_loop(
            |input, retry_errors| {
                async move {
                    // spin 32x
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop(); std::hint::spin_loop();
                    Ok((input, retry_errors))
                }
            },
            |_input, _error, _retry_errors_list| future::ready(())
        ).await
    }

    /// Upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], executing the `retry_operation`
    /// until it either succeeds or fatably fails, suggesting that `tokio` should run other tasks in-between.\
    /// Use with caution, as this method may dead-lock the thread, at 100% CPU usage, as there is no limit for the number of retries.\
    /// See also [Self::with_delays()], [Self::spinning_forever()]
    pub async fn yielding_forever(self) -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        self.retry_loop(
            |input, retry_errors| {
                async move {
                    tokio::task::yield_now().await;
                    Ok((input, retry_errors))
                }
            },
            |_input, _error, _retry_errors_list| future::ready(())
        ).await
    }

    /// Upgrades this [KeenRetryAsyncExecutor] into the final [ResolvedResult], executing the `retry_operation`
    /// until it either succeeds (or fatably fails) or `timeout` elapses -- in which case, the operation will fail
    /// with the error given by `timeout_error_generator()`.\
    /// Upon each new attempt, control will be passed to `tokio` so other tasks may run.
    pub async fn yielding_until_timeout(self,
                                        timeout:                 Duration,
                                        timeout_error_generator: impl Fn() -> ErrorType)
                                       -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {
        let start = SystemTime::now();
        self.retry_loop(
            |input, retry_errors| {
                let timeout_error_generator = &timeout_error_generator;
                async move {
                    tokio::task::yield_now().await;
                    if start.elapsed().unwrap_or_default() >= timeout {
                        Err(ResolvedResult::GivenUp { input, retry_errors, fatal_error: timeout_error_generator() })
                    } else {
                        Ok((input, retry_errors))
                    }
                }
            },
            |_input, _error, _retry_errors_list| future::ready(())
        ).await
    }

    /// The low-level generic retry loop for this async executor -- for when [Self::with_delays()], [Self::spinning_forever()],
    /// [Self::yielding_forever()] nor [Self::yielding_until_timeout()] suit your needs:
    ///   - `on_pre_reattempt(&input, &mut retry_errors_list) -> Result<(), ErrorType>`
    ///     Called before performing (another) retry attempt. If it returns a non-`Ok` value, the returned error
    ///     will be reported as `fatal` and the `retry_loop()` will end.
    ///   - `on_non_fatal_failure(&input, error, &mut retry_errors_list)`
    ///     Called when the current retry attempt failed. Used to, optionally, push the error in the `retry_errors_list`.
    /// See the sources of [Self::with_delays()] for a good example of how to use this low level function.
    pub async fn retry_loop<OnPreReattemptFuture:    Future<Output=Result<(OriginalInput, Vec<ErrorType>), ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType>>>,
                            OnNonFatalFailureFuture: Future<Output=()>>

                           (self,
                            mut on_pre_attempt:       impl FnMut(OriginalInput,  Vec<ErrorType>)                 -> OnPreReattemptFuture,
                            mut on_non_fatal_failure: impl FnMut(&OriginalInput, ErrorType, &mut Vec<ErrorType>) -> OnNonFatalFailureFuture)

                           -> ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType> {

        match self {
            KeenRetryAsyncExecutor::ResolvedOk  { reported_input, output } => ResolvedResult::from_ok_result(reported_input, output),
            KeenRetryAsyncExecutor::ResolvedErr { original_input, error } => ResolvedResult::from_err_result(original_input, error),
            KeenRetryAsyncExecutor::ToRetry { mut input, mut retry_async_operation, mut retry_errors } => {
                loop {
                    (input, retry_errors) = match on_pre_attempt(input, retry_errors).await {
                        Ok((input, mut retry_errors)) => {
                            let new_retry_result = retry_async_operation(input).await;
                            match new_retry_result {
                                RetryResult::Ok    { reported_input, output } => break ResolvedResult::Recovered { reported_input, output, retry_errors },
                                RetryResult::Fatal { input, error }          => break ResolvedResult::Unrecoverable { input, retry_errors, fatal_error: error },
                                RetryResult::Transient { input, error }          => {
                                    on_non_fatal_failure(&input, error, &mut retry_errors).await;
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

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType,
     AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
     OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>>
From<KeenRetryAsyncExecutor<ReportedInput,
                            OriginalInput,
                            Output,
                            ErrorType,
                            AsyncRetryFn,
                            OutputFuture>> for
RetryResult<ReportedInput,
            OriginalInput,
            Output,
            ErrorType> {

    /// Allows compile-time transformation of a "unexecuted" keen retry executor back into its original `RetryResult`, enabling extra flexibility
    /// in patterns like the following -- from the `reactive-messaging` crate:
    /// ```nocompile
    ///         let retryable = retry_result_supplier(SystemTime::now()).await  // this is the common code -- the same for
    ///             .retry_with_async(retry_result_supplier);                   // the cases with and without retrying bellow
    ///         let resolved_result = match config.retrying_strategy {
    ///             RetryingStrategies::DoNotRetry =>
    ///                 ResolvedResult::from_retry_result(retryable.into()),  // flexibly bail out the executor
    ///                                                                       // (enabling the first line as the common code for all cases)
    ///             RetryingStrategies::RetryWithBackoffUpTo(attempts) =>
    ///                 retryable
    ///                     .with_exponential_jitter(|| ExponentialJitter::FromBackoffRange {
    ///                         backoff_range_millis: 1..=(2.526_f32.powi(attempts as i32) as u32),
    ///                         re_attempts: attempts,
    ///                         jitter_ratio: 0.2,
    ///                     })
    ///                     .await,
    /// 
    ///             RetryingStrategies::RetryYieldingForUpToMillis(millis) =>
    ///                 retryable
    ///                     .yielding_until_timeout(Duration::from_millis(millis as u64), || Box::from(format!("Timed out (>{millis}ms) while attempting to connect to {}:{}", self.host, self.port)))
    ///                     .await,
    ///         };
    #[inline(always)]
    fn from(executor: KeenRetryAsyncExecutor<ReportedInput, OriginalInput, Output, ErrorType, AsyncRetryFn, OutputFuture>) -> Self {
        match executor {
            KeenRetryAsyncExecutor::ResolvedOk  { reported_input, output }                                     => RetryResult::Ok        { reported_input, output },
            KeenRetryAsyncExecutor::ToRetry     { input, retry_async_operation: _, mut retry_errors }  => RetryResult::Transient { input, error: retry_errors.pop().expect("BUG (popping)") },
            KeenRetryAsyncExecutor::ResolvedErr { original_input, error }                                   => RetryResult::Fatal     { input: original_input, error }
        }
    }
}