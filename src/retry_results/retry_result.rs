//! Resting place for [RetryResult]


use std::future::Future;
use crate::keen_retry_async_executor::KeenRetryAsyncExecutor;
use crate::keen_retry_executor::KeenRetryExecutor;

/// Wrapper for the return type of fallible & retryable functions -- an extension for `Result<OkPayload, ErrorType>`,
/// but also accepting an `input`.\
/// Considering zero-copy, both `Retry` & `Fatal` variants will contain the original input payload, which is consumed by an `Ok` operation;
/// The `Ok` operation, on the other hand, has the outcome result.
pub enum RetryResult<ReportedInput,
                     OriginalInput,
                     Output,
                     ErrorType> {
    Ok {
        reported_input: ReportedInput,
        output:         Output,
    },

    Retry {
        input: OriginalInput,
        error: ErrorType,
    },

    Fatal {
        input: OriginalInput,
        error: ErrorType,
    },
}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType>
RetryResult<ReportedInput,
            OriginalInput,
            Output,
            ErrorType> {

    pub fn inspect_ok<IgnoredReturn,
                                  F: FnOnce(&ReportedInput, &Output) -> IgnoredReturn>
                                 (self, f: F) -> Self {
        if let Self::Ok { ref reported_input, ref output } = self {
            f(reported_input, output);
        }
        self
    }

    pub fn inspect_retry<IgnoredReturn,
                         F: FnOnce(&OriginalInput, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Retry { input: ref payload, ref error } = self {
            f(payload, error);
        }
        self
    }

    pub fn inspect_fatal<IgnoredReturn,
                         F: FnOnce(&OriginalInput, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Fatal { input: ref payload, ref error } = self {
            f(payload, error);
        }
        self
    }

    pub fn map_ok<NewReportedInput,
                  NewOutput,
                  F: FnOnce(ReportedInput, Output) -> (NewReportedInput, NewOutput)>
                 (self, f: F) -> RetryResult<NewReportedInput, OriginalInput, NewOutput, ErrorType> {
        match self {
            RetryResult::Retry { input, error }          => RetryResult::Retry { input, error },
            RetryResult::Fatal { input, error }          => RetryResult::Fatal { input, error },
            RetryResult::Ok    { reported_input, output } => {
                let (reported_input, output) = f(reported_input, output);
                RetryResult::Ok    { reported_input, output }
            },
        }
    }

    pub fn map_input<NewOriginalInput,
                     F: FnOnce(OriginalInput) -> NewOriginalInput>
                    (self, f: F) -> RetryResult<ReportedInput, NewOriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok    { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Retry { input, error }          => RetryResult::Retry { input: f(input), error },
            RetryResult::Fatal { input, error }          => RetryResult::Fatal { input: f(input), error },
        }
    }

    pub fn map_retry_error<F: FnOnce(ErrorType) -> ErrorType>
                          (self, f: F) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok    { reported_input, output }  => RetryResult::Ok    { reported_input, output },
            RetryResult::Retry { input, error }           => RetryResult::Retry { input, error: f(error) },
            RetryResult::Fatal { input, error }           => RetryResult::Fatal { input, error },
        }
    }

    pub fn map_fatal_error<F: FnOnce(ErrorType) -> ErrorType>
                          (self, f: F) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok    { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Retry { input, error }          => RetryResult::Retry { input, error },
            RetryResult::Fatal { input, error }          => RetryResult::Fatal { input, error: f(error) },
        }
    }

    /// Upgrades this [RetryResult] into a [KeenRetryExecutor], which will, on its turn, be upgraded to [ResolvedResult], containing the final results of the retryable operation
    pub fn retry_with<RetryFn: FnMut(OriginalInput) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>
                     (self,
                      retry_operation: RetryFn)
                     -> KeenRetryExecutor<ReportedInput, OriginalInput, Output, ErrorType, RetryFn> {

        match self {
            RetryResult::Ok    { reported_input, output } => KeenRetryExecutor::from_resolved(Ok(output)),
            RetryResult::Fatal { input, error }          => KeenRetryExecutor::from_resolved(Err(error)),
            RetryResult::Retry { input, error }          => KeenRetryExecutor::new(input, retry_operation, error),
        }
    }

    /// Upgrades this [RetryResult] into a [KeenRetryExecutor], which will, on its turn, be upgraded to [ResolvedResult], containing the final results of the retryable operation
    pub fn retry_with_async<AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
                            OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>>
                           (self,
                            retry_operation: AsyncRetryFn)
                           -> KeenRetryAsyncExecutor<ReportedInput, OriginalInput, Output, ErrorType, AsyncRetryFn, OutputFuture> {

        match self {
            RetryResult::Ok    { reported_input, output } => KeenRetryAsyncExecutor::from_resolved(Ok(output)),
            RetryResult::Fatal { input, error }          => KeenRetryAsyncExecutor::from_resolved(Err(error)),
            RetryResult::Retry { input, error }          => KeenRetryAsyncExecutor::new(input, retry_operation, error),
        }
    }

}