//! Resting place for [RetryResult] and associated sugar types [RetryProcedureResult], [RetryConsumerResult] and [RetryProcedureResult].

use crate::{
    keen_retry_executor::KeenRetryExecutor,
    keen_retry_async_executor::KeenRetryAsyncExecutor,
};
use std::future::Future;


// Some suggar types:
// //////////////////

/// Suggar type for when an operation doesn't consume its inputs nor produce outputs
pub type RetryProcedureResult<ErrorType> = RetryResult<(), (), (), ErrorType>;

/// Suggar type for when an operation doesn't produce outputs
pub type RetryConsumerResult<ReportedInput, OriginalInput, ErrorType> = RetryResult<ReportedInput, OriginalInput, (), ErrorType>;

/// Suggar type for when an operation doesn't consume its inputs
pub type RetryProducerResult<Output, ErrorType> = RetryResult<(), (), Output, ErrorType>;


/// An extension over the original std `Result<Ok, Err>`, introducing a third kind: Transient failures
/// -- which are elligible for retry attempts: this may be considered the "First Level" of results,
/// mapping directly from raw operation results.\
/// Considering zero-copy, both `Transient` & `Fatal` variants will contain the original input payload, which is consumed by an `Ok` operation;
/// The `Ok` operation, on the other hand, has the outcome result and may have an excerpt of the input, for instrumentation purposes.\
/// See also [crate::RetryResult], for the "Second Level" of results -- after passing through some possible retry re-attempts.
pub enum RetryResult<ReportedInput,
                     OriginalInput,
                     Output,
                     ErrorType> {

    /// Represents the result of an operation that succeeded.\
    /// Maps to a `Result::Ok`.
    Ok {
        reported_input: ReportedInput,
        output:         Output,
    },

    /// Represents the result of an operation that faced a transient error
    /// -- which are elligible for retrying.\
    /// Maps to a `Result::Err` if not opting-in for the retrying procedure.
    Transient {
        input: OriginalInput,
        error: ErrorType,
    },

    /// Represents the result of an operation that faced a fatal error.\
    /// Maps to a `Result::Err`.
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

    /// If this operation is [Self::Ok], spots its data by calling `f(&reported_input, &output)`
    #[inline(always)]
    pub fn inspect_ok<IgnoredReturn,
                                  F: FnOnce(&ReportedInput, &Output) -> IgnoredReturn>
                                 (self, f: F) -> Self {
        if let Self::Ok { ref reported_input, ref output } = self {
            f(reported_input, output);
        }
        self
    }

    /// If this operation is [Self::Transient], spots its data by calling `f(&original_input, &error)`
    #[inline(always)]
    pub fn inspect_transient<IgnoredReturn,
                             F: FnOnce(&OriginalInput, &ErrorType) -> IgnoredReturn>
                            (self, f: F) -> Self {
        if let Self::Transient { input: ref payload, ref error } = self {
            f(payload, error);
        }
        self
    }

    /// Spots on the data of an operation that failed fatably (in which case, no retrying will be attempted).
    ///   - `f(&original_input, &error_type)`
    #[inline(always)]
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
    /// See [Self::map_ok()] if you'd rather change the "reported input" & output of an operation that succeeded.
    ///
    /// A nice usage would be to upgrade the initial payload to a tuple, keeping track of how much time will be spent retrying:
    /// ```nocompile
    ///     .map_input(|payload| (payload, SystemTime::now()))
    #[inline(always)]
    pub fn map_input<NewOriginalInput,
                     F: FnOnce(OriginalInput) -> NewOriginalInput>
                    (self, f: F) -> RetryResult<ReportedInput, NewOriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok        { reported_input, output } => RetryResult::Ok        { reported_input, output },
            RetryResult::Transient { input, error }        => RetryResult::Transient { input: f(input), error },
            RetryResult::Fatal     { input, error }        => RetryResult::Fatal     { input: f(input), error },
        }
    }

    /// Changes the input & output data associated with an operation that was successful.
    ///   - `f(reported_input, output) -> (new_reported_input, new_output)`.\
    /// See [Self::map_input()] if you'd rather change original input instead
    #[inline(always)]
    pub fn map_ok<NewReportedInput,
                  NewOutput,
                  F: FnOnce(ReportedInput, Output) -> (NewReportedInput, NewOutput)>
                 (self, f: F) -> RetryResult<NewReportedInput, OriginalInput, NewOutput, ErrorType> {
        match self {
            RetryResult::Transient { input, error }        => RetryResult::Transient { input, error },
            RetryResult::Fatal     { input, error }        => RetryResult::Fatal     { input, error },
            RetryResult::Ok        { reported_input, output } => {
                let (reported_input, output) = f(reported_input, output);
                RetryResult::Ok    { reported_input, output }
            },
        }
    }

    /// Changes the (value of the) error that indicates this operation may be retried.\
    /// See [Self::map_errors()] if you'd also like to change the type;\
    /// See [Self::map_inputs_and_errors()] if you'd like to remap/swap all possible pairs of input/error
    #[inline(always)]
    pub fn map_transient_error<F: FnOnce(ErrorType) -> ErrorType>
                              (self, f: F)
                              -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok        { reported_input, output }  => RetryResult::Ok        { reported_input, output },
            RetryResult::Transient { input, error }         => RetryResult::Transient { input, error: f(error) },
            RetryResult::Fatal     { input, error }         => RetryResult::Fatal     { input, error },
        }
    }

    /// Changes the (value of the) error that indicates this operation may not be retried.\
    /// See [Self::map_errors()] if you'd also like to change the type.
    #[inline(always)]
    pub fn map_fatal_error<F: FnOnce(ErrorType) -> ErrorType>
                          (self, f: F)
                          -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            RetryResult::Ok        { reported_input, output } => RetryResult::Ok        { reported_input, output },
            RetryResult::Transient { input, error }        => RetryResult::Transient { input, error },
            RetryResult::Fatal     { input, error }        => RetryResult::Fatal     { input, error: f(error) },
        }
    }

    /// Allows changing the (original input,error) pairs for both error possibilities:
    ///   - fatal_map_fn(input, fatal_error)         -> (new_input, new_fatal_error)
    ///   - transient_map_fn(input, transient_error) -> (new_input, new_transient_error)
    /// 
    /// Covers the case where it is desireable to bail out the retrying process of a consumer operation,
    /// moving the consumed input back to the error, so the caller may not lose the payload.
    /// 
    /// See also [ResolvedResult::map_inputs_and_errors()] for similar semantics after exhausting retries.
    #[inline(always)]
    pub fn map_input_and_errors<NewOriginalInput,
                                 NewErrorType,
                                 FatalMapFn:     FnOnce(OriginalInput, ErrorType) -> (NewOriginalInput, NewErrorType),
                                 TransientMapFn: FnOnce(OriginalInput, ErrorType) -> (NewOriginalInput, NewErrorType)>

                                (self,
                                 fatal_map_fn:     FatalMapFn,
                                 transient_map_fn: TransientMapFn)

                                -> RetryResult<ReportedInput, NewOriginalInput, Output, NewErrorType> {
        match self {
            RetryResult::Ok        { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Transient { input, error }        => {
                let (new_input, new_error) = transient_map_fn(input, error);
                RetryResult::Transient { input: new_input, error: new_error }
            },
            RetryResult::Fatal { input, error } => {
                let (new_input, new_error) = fatal_map_fn(input, error);
                RetryResult::Fatal { input: new_input, error: new_error }
            },
        }
    }

    /// TODO: add the docs
    #[inline(always)]
    pub fn or_else_with<F: FnOnce(OriginalInput, ErrorType) -> Self>
                       (self, f: F)
                       -> Self {
        match self {
            RetryResult::Transient { input, error }   => f(input, error),
            RetryResult::Ok { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Fatal { input, error }       => RetryResult::Fatal { input, error },
        }
    }

    /// TODO: add the docs
    #[inline(always)]
    pub async fn or_else_with_async<F:            FnOnce(OriginalInput, ErrorType) -> OutputFuture,
                                    OutputFuture: Future<Output=Self>>
                                   (self, f: F)
                                   -> Self {
        match self {
            RetryResult::Transient { input, error }   => f(input, error).await,
            RetryResult::Ok { reported_input, output } => RetryResult::Ok    { reported_input, output },
            RetryResult::Fatal { input, error }       => RetryResult::Fatal { input, error },
        }
    }

    /// TODO: add the docs
    #[inline(always)]
    pub fn or_else_with_supplier<RetryFnOutput,
                                 LibraryRetryFn: FnMut() -> RetryFnOutput>
                                ()
                                -> RetryResult<ReportedInput, LibraryRetryFn, Output, ErrorType> {
        todo!()
    }

    /// Upgrades this [RetryResult] into a [KeenRetryExecutor], which will, on its turn, be upgraded to [crate::ResolvedResult],
    /// containing the final results after executing the retrying process
    #[inline(always)]
    pub fn retry_with<RetryFn: FnMut(OriginalInput) -> RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>
                     (self,
                      retry_operation: RetryFn)
                     -> KeenRetryExecutor<ReportedInput, OriginalInput, Output, ErrorType, RetryFn> {

        match self {
            RetryResult::Ok        { reported_input, output } => KeenRetryExecutor::from_ok_result(reported_input, output),
            RetryResult::Fatal     { input, error }        => KeenRetryExecutor::from_err_result(input, error),
            RetryResult::Transient { input, error }        => KeenRetryExecutor::new(input, retry_operation, error),
        }
    }

    /// Upgrades this [RetryResult] into a [KeenRetryAsyncExecutor], which will, on its turn, be upgraded to [crate::ResolvedResult],
    /// containing the final results after executing the retrying process
    #[inline(always)]
    pub fn retry_with_async<AsyncRetryFn: FnMut(OriginalInput) -> OutputFuture,
                            OutputFuture: Future<Output=RetryResult<ReportedInput, OriginalInput, Output, ErrorType>>>
                           (self,
                            retry_operation: AsyncRetryFn)
                           -> KeenRetryAsyncExecutor<ReportedInput, OriginalInput, Output, ErrorType, AsyncRetryFn, OutputFuture> {

        match self {
            RetryResult::Ok        { reported_input, output } => KeenRetryAsyncExecutor::from_ok_result(reported_input, output),
            RetryResult::Fatal     { input, error }        => KeenRetryAsyncExecutor::from_err_result(input, error),
            RetryResult::Transient { input, error }        => KeenRetryAsyncExecutor::new(input, retry_operation, error),
        }
    }

    /// Returns `true` if the operation succeeded.\
    /// Also mimmics the `Result<>` API for users that don't opt-in for the `keen-retry` API.
    #[inline(always)]
    pub fn is_ok(&self) -> bool {
        matches!(self, RetryResult::Ok {..})
    }

    /// Mimmics the `Result<>` API for users that don't opt-in for the `keen-retry` API:
    /// simply returns `true` if the error is [Self::Fatal] or [Self::Transient].
    #[inline(always)]
    pub fn is_err(&self) -> bool {
        self.is_transient() || self.is_fatal()
    }

    /// Returns `true` if the operation failed fatably
    #[inline(always)]
    pub fn is_fatal(&self) -> bool {
        matches!(self, RetryResult::Fatal {..})
    }

    /// Returns `true` if the operation resulted in a transient error,
    /// electing the same input(s) for a retry.\
    #[inline(always)]
    pub fn is_transient(&self) -> bool {
        matches!(self, RetryResult::Transient {..})
    }

    /// Panics if `self` isn't [RetryResult::Ok]
    #[inline(always)]
    pub fn expect_ok(&self, panic_msg: &str) {
        if !self.is_ok() {
            panic!("{panic_msg}")
        }
    }

    /// Panics if `self` isn't [RetryResult::Transient]
    #[inline(always)]
    pub fn expect_transient(&self, panic_msg: &str) {
        if !self.is_transient() {
            panic!("{panic_msg}")
        }
    }

    /// Panics if `self` isn't [RetryResult::Fatal]
    #[inline(always)]
    pub fn expect_fatal(&self, panic_msg: &str) {
        if !self.is_fatal() {
            panic!("{panic_msg}")
        }
    }

    /// Syntatic sugar for [Result<Output, ErrorType>::from()].\
    /// See also [Self::into()]
    #[inline(always)]
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

    /// Opts out of any retrying attempts and downgrades this, potentially retryable result, into a standard Rust `Result<>`.\
    /// To opt-in for the retrying process, see [RetryResult::retry_with()] or [RetryResult::retry_with_async()]
    #[inline(always)]
    fn from(retry_result: RetryResult<ReportedInput, OriginalInput, Output, ErrorType>) -> Self {
        match retry_result {
            RetryResult::Ok { reported_input: _, output }    => Ok(output),
            RetryResult::Fatal { input: _, error }        => Err(error),
            RetryResult::Transient { input: _, error }    => Err(error),
        }
    }
}