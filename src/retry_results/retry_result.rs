//! Resting place for [RetryResult]


use crate::{
    keen_retry_executor::KeenRetryExecutor,
    keen_retry_async_executor::KeenRetryAsyncExecutor,
};
use std::future::Future;

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

    /// Spots on the data of an operation that failed fatably on the first shot (in which case, no retrying will be attempted).
    ///   - `f(&original_input, &error_type)`
    pub fn inspect_fatal<IgnoredReturn,
                         F: FnOnce(&OriginalInput, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Fatal { ref input, ref error } = self {
            f(input, error);
        }
        self
    }

    /// Changes the original input data to be re-fed to an operation that failed and will be retried.
    ///   - `f(original_input) -> new_original_input`.\
    /// See [Self::map_ok()] if you'd rather change the "reported input" & output of an operation that succeeded/failed fatably.
    ///
    /// A nice usage would be to upgrade the initial payload to a tuple, keeping track of how much time will be spent retrying:
    /// ```nocompile
    ///     .map_input(|payload| (payload, SystemTime::now()))
    pub fn map_input<NewOriginalInput,
                     F: FnOnce(OriginalInput) -> NewOriginalInput>
                    (self, f: F) -> RetryResult<ReportedInput, NewOriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok    { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Retry { input, error }          => RetryResult::Retry { input: f(input), error },
            RetryResult::Fatal { input, error }          => RetryResult::Fatal { input: f(input), error },
        }
    }

    /// Changes the input & output data associated with an operation that was successful at the first shot.
    ///   - `f(reported_input, output) -> (new_reported_input, new_output)`.\
    /// See [Self::map_input()] if you'd rather change original input instead
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

    /// Changes the (value of the) error that indicates this operation may be retried.\
    /// See [Self::map_errors()] if you'd also like to change the type;\
    /// See [Self::map_inputs_and_errors()] if you'd like to remap/swap all possible pairs of input/error
    pub fn map_retry_error<F: FnOnce(ErrorType) -> ErrorType>
                          (self, f: F) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok    { reported_input, output }  => RetryResult::Ok    { reported_input, output },
            RetryResult::Retry { input, error }           => RetryResult::Retry { input, error: f(error) },
            RetryResult::Fatal { input, error }           => RetryResult::Fatal { input, error },
        }
    }

    /// Changes the (value of the) error that indicates this operation may not be retried.\
    /// See [Self::map_errors()] if you'd also like to change the type.
    pub fn map_fatal_error<F: FnOnce(ErrorType) -> ErrorType>
                          (self, f: F) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok    { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Retry { input, error }          => RetryResult::Retry { input, error },
            RetryResult::Fatal { input, error }          => RetryResult::Fatal { input, error: f(error) },
        }
    }

    /// Allows changing the (input,error) pairs for both error possibilities:
    ///   - retry_map_fn(input, retry_error)       -> (new_input, new_retry_error)
    ///   - fatal_error_map_fn(input, fatal_error) -> (new_input, new_fatal_error)
    /// 
    /// Covers the case where it is desireable to bail out the retrying process of a consumer operation,
    /// moving the consumed input back to the error, so the caller may not lose the payload.
    pub fn map_inputs_and_errors<NewOriginalInput,
                                 NewErrorType,
                                 RetryMapFn: FnOnce(OriginalInput, ErrorType) -> (NewOriginalInput, NewErrorType),
                                 FatalMapFn: FnOnce(OriginalInput, ErrorType) -> (NewOriginalInput, NewErrorType)>

                                (self,
                                 retry_map_fn: RetryMapFn,
                                 fatal_map_fn: FatalMapFn)

                                -> RetryResult<ReportedInput, NewOriginalInput, Output, NewErrorType> {
        match self {
            RetryResult::Ok    { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Retry { input, error } => {
                let (new_input, new_error) = retry_map_fn(input, error);
                RetryResult::Retry { input: new_input, error: new_error }
            },
            RetryResult::Fatal { input, error } => {
                let (new_input, new_error) = fatal_map_fn(input, error);
                RetryResult::Fatal { input: new_input, error: new_error }
            },
        }
    }

    /// Upgrades this [RetryResult] into a [KeenRetryExecutor], which will, on its turn, be upgraded to [ResolvedResult], containing the final results after executing the retryable operation
    pub fn retry_with<RetryFn: FnMut(OriginalInput) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>
                     (self,
                      retry_operation: RetryFn)
                     -> KeenRetryExecutor<ReportedInput, OriginalInput, Output, ErrorType, RetryFn> {

        match self {
            RetryResult::Ok    { reported_input, output } => KeenRetryExecutor::from_ok_result(reported_input, output),
            RetryResult::Fatal { input, error }          => KeenRetryExecutor::from_err_result(input, error),
            RetryResult::Retry { input, error }          => KeenRetryExecutor::new(input, retry_operation, error),
        }
    }

    /// Upgrades this [RetryResult] into a [KeenRetryAsyncExecutor], which will, on its turn, be upgraded to [ResolvedResult], containing the final results after executing the retryable operation
    pub fn retry_with_async<AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
                            OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>>
                           (self,
                            retry_operation: AsyncRetryFn)
                           -> KeenRetryAsyncExecutor<ReportedInput, OriginalInput, Output, ErrorType, AsyncRetryFn, OutputFuture> {

        match self {
            RetryResult::Ok    { reported_input, output } => KeenRetryAsyncExecutor::from_ok_result(reported_input, output),
            RetryResult::Fatal { input, error }          => KeenRetryAsyncExecutor::from_err_result(input, error),
            RetryResult::Retry { input, error }          => KeenRetryAsyncExecutor::new(input, retry_operation, error),
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, RetryResult::Ok {..})
    }

    pub fn is_fatal(&self) -> bool {
        matches!(self, RetryResult::Fatal {..})
    }

    pub fn is_retry(&self) -> bool {
        matches!(self, RetryResult::Retry {..})
    }

    pub fn expect_ok(&self, panic_msg: &str) {
        if !self.is_ok() {
            panic!("{panic_msg}")
        }
    }

    pub fn expect_fatal(&self, panic_msg: &str) {
        if !self.is_fatal() {
            panic!("{panic_msg}")
        }
    }

    pub fn expect_retry(&self, panic_msg: &str) {
        if !self.is_retry() {
            panic!("{panic_msg}")
        }
    }

    /// Syntatic sugar for [Result<Output, ErrorType>::from()].\
    /// See also [Self::into()]
    pub fn into_result(self) -> Result::<Output, ErrorType> {
        Result::<Output, ErrorType>::from(self)
    }

}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType>
From<RetryResult<ReportedInput,
     OriginalInput,
     Output,
     ErrorType>> for
Result<Output, ErrorType> {

    /// Opts out of any retrying attempts and converts the "first shot" of a retryable operation into a `Result<>`.\
    /// To opt-in the retrying process, see [RetryResult::retry_with()] or [RetryResult::retry_with_async()]
    fn from(retry_result: RetryResult<ReportedInput, OriginalInput, Output, ErrorType>) -> Self {
        match retry_result {
            RetryResult::Ok { reported_input: _, output }            => Ok(output),
            RetryResult::Fatal { input: _, error }                => Err(error),
            RetryResult::Retry { input: _, error }                => Err(error),
        }
    }
}