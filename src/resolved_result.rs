//! Resting place for [ResolvedResult], representing the final outcome of a retryable operation


/// Contains all possibilities for finished retryable operations -- conversible to `Result<>` --
/// and some nice facilities for instrumentation (like building a succinct report of the retry errors).\
/// This "Final Result" is a "Second Level" of result for an operation: it represents operations that
/// where enabled to pass through the `keen-retry` retrying logic.\
/// See also [crate::RetryResult], for the "First Level" of results.
pub enum ResolvedResult<ReportedInput,
                        OriginalInput,
                        Output,
                        ErrorType> {

    /// Represents the result of an operation that succeeded at the first attempt.\
    /// Maps to a `Result::Ok`.
    Ok {
        reported_input: ReportedInput,
        output:         Output,
    },

    /// Represents the result of an operation that failed fatably at the first attempt.\
    /// Maps to a `Result::Err`.
    Fatal {
        input: OriginalInput,
        error: ErrorType,
    },

    /// Represents the result of an operation that failed transiently at the first
    /// attempt, but succeeded at a subsequent retry.\
    /// Maps to a `Result::Ok`
    Recovered {
        reported_input: ReportedInput,
        output:         Output,
        retry_errors:   Vec<ErrorType>,
    },

    /// Represents the result of an operation that failed transiently at the first
    /// and all subsequent retry attempts -- exceeding the retry limit.\
    /// Maps to a `Result::Err`
    GivenUp {
        input:        OriginalInput,
        retry_errors: Vec<ErrorType>,
        fatal_error:  ErrorType,
    },

    /// Represents the result of an operation that failed transiently at the first
    /// attempt and, on one of the retry re-attempts, faced a fatal error.\
    /// Maps to a `Result::Err`
    Unrecoverable {
        input:        OriginalInput,
        retry_errors: Vec<ErrorType>,
        fatal_error:  ErrorType,
    },

}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType>

ResolvedResult<ReportedInput,
               OriginalInput,
               Output,
               ErrorType> {

    /// Builds an instance with the results of an operation that suceeded at the first attempt.
    ///   * `output` represents the data returned by the operation.
    ///   * `reported_input` represents the input of the operation -- since the operation may
    ///     consume the original input, `reported_input` may be just a light-weight representation
    ///     of it, possibly for instrumentation.
    pub fn from_ok_result(reported_input: ReportedInput, output: Output) -> Self {
        ResolvedResult::Ok { reported_input, output }
    }

    /// Builds an instance with the results of an operation that failed fatably at the first attempt.
    ///   * `error` specifies the fatal, unrecoverable failure.
    ///   * `original_input` is the data that the operation would consume, if successful.
    pub fn from_err_result(original_input: OriginalInput, error: ErrorType) -> Self {
        ResolvedResult::Fatal { input: original_input, error }
    }

    /// Tells whether this result represents one of the successful variants
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Ok { .. } | Self::Recovered { .. })
    }

    /// Tells whether this result represents one of the failure variants
    pub fn is_failure(&self) -> bool {
        !self.is_success()
    }

    /// If this operation is [Self::Ok], spots its data by calling `f(&reported_input, &output)`
    pub fn inspect_ok<IgnoredReturn,
                      F: FnOnce(&ReportedInput, &Output) -> IgnoredReturn>
                     (self, f: F) -> Self {
        if let Self::Ok { ref reported_input, ref output } = self {
            f(reported_input, output);
        }
        self
    }

    /// If this operation is [Self::Fatal], spots its data by calling `f(&original_input, &error)`
    pub fn inspect_fatal<IgnoredReturn,
                         F: FnOnce(&OriginalInput, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Fatal { ref input, ref error } = self {
            f(input, error);
        }
        self
    }

    /// Spots on the data of an already resolved (and successful) operation that required some retry attempts to succeed.
    ///   - `f(&reported_input, &output, &retry_errors_list)`
    ///   - if the error type implements `Debug`, you can use `let loggable_retry_errors = keen_retry::loggable_retry_errors(&retry_errors_list)`
    ///     inside `f()` to serialize the `retry_errors_list` into a loggable String
    pub fn inspect_recovered<IgnoredReturn,
                             F: FnOnce(&ReportedInput, &Output, &Vec<ErrorType>) -> IgnoredReturn>
                            (self, f: F) -> Self {
        if let Self::Recovered { ref reported_input, ref output, ref retry_errors } = self {
            f(reported_input, output, retry_errors);
        }
        self
    }

    /// Spots on the data of an already resolved (but unsuccessful) operation that got retried as much as it could.
    ///   - `f(&original_input, &retry_errors_list, &fatal_error)`
    ///   - if the error type implements `Debug`, you can do `let loggable_retry_errors = keen_retry::loggable_retry_errors(&retry_errors_list)`
    ///     inside `f()` to serialize the `retry_errors_list` into a loggable String
    pub fn inspect_given_up<IgnoredReturn,
                            F: FnOnce(&OriginalInput, &Vec<ErrorType>, /*fatal_error: */&ErrorType) -> IgnoredReturn>
                           (self, f: F) -> Self {
        if let Self::GivenUp { ref input, ref retry_errors, ref fatal_error } = self {
            f(input, retry_errors, fatal_error);
        }
        self
    }

    /// Spots on the data of an already resolved (but unsuccessful) operation that failed fatably in one of its retry attempts.
    ///   - `f(&original_input, &retry_errors_list, &fatal_error)`
    ///   - if the error type implements `Debug`, you can do `let loggable_retry_errors = keen_retry::loggable_retry_errors(&retry_errors_list)`
    ///     inside `f()` to serialize the `retry_errors_list` into a loggable String
    pub fn inspect_unrecoverable<IgnoredReturn,
                                 F: FnOnce(&OriginalInput, &Vec<ErrorType>, /*fatal_error: */&ErrorType) -> IgnoredReturn>
                                (self, f: F) -> Self {
        if let Self::Unrecoverable { ref input, ref retry_errors, ref fatal_error } = self {
            f(input, retry_errors, fatal_error);
        }
        self
    }

    /// Allows changing the reported input data of an already resolved (and successful) operation.\
    /// A successful operation is likely to consume the input and the `reported_input` is expected to be a 
    /// light-weight representation of it -- lets say, for instrumentation purposes -- so a copy doesn't need to be made.\
    /// This method allows a laden `reported_input` to be downgraded to its original form, where:
    ///   - `f(upgraded_reported_input) -> original_reported_input`.
    ///
    /// Example:
    /// ```nocompile
    ///     // downgrades the laden `reported_input` that was useful during logging & retrying
    ///     .map_reported_input(|(loggable_payload, duration)| loggable_payload)
    pub fn map_reported_input<NewReportedInput,
                              F: FnOnce(ReportedInput) -> NewReportedInput>
                             (self, f: F) -> ResolvedResult<NewReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            ResolvedResult::Ok            { reported_input, output }                                => ResolvedResult::Ok            { reported_input: f(reported_input), output },
            ResolvedResult::Fatal         { input, error}                                        => ResolvedResult::Fatal         { input, error },
            ResolvedResult::Recovered     { reported_input, output, retry_errors }  => ResolvedResult::Recovered     { reported_input: f(reported_input), output, retry_errors },
            ResolvedResult::GivenUp       { input, retry_errors, fatal_error }   => ResolvedResult::GivenUp       { input, retry_errors, fatal_error },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }   => ResolvedResult::Unrecoverable { input, retry_errors, fatal_error },
        }
    }

    /// Changes the original input data of an already resolved (but unsuccessful) operation.
    ///   - `f(upgraded_original_input) -> original_input`.
    ///
    /// A possible usage would be to downgrade a laden payload to its initial form, so it may be returned back to the caller:
    /// ```nocompile
    ///     // downgrades the laden payload that was useful when logging the retry attempts & outcomes
    ///     .map_unrecoverable_input(|(original_payload, duration)| original_payload)
    pub fn map_unrecoverable_input<NewOriginalInput,
                                   F: FnOnce(OriginalInput) -> NewOriginalInput>
                                  (self, f: F) -> ResolvedResult<ReportedInput, NewOriginalInput, Output, ErrorType> {
        match self {
            ResolvedResult::Ok            { reported_input, output }                               => ResolvedResult::Ok            { reported_input, output },
            ResolvedResult::Fatal         { input, error }                                      => ResolvedResult::Fatal         { input: f(input), error },
            ResolvedResult::Recovered     { reported_input, output, retry_errors } => ResolvedResult::Recovered     { reported_input, output, retry_errors },
            ResolvedResult::GivenUp       { input, retry_errors, fatal_error }  => ResolvedResult::GivenUp       { input: f(input), retry_errors, fatal_error },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }  => ResolvedResult::Unrecoverable { input: f(input), retry_errors, fatal_error },
        }
    }

    /// Allows changing the `reported_input` and `output` of an already resolved (and successful) operation.\
    ///   - `f(reported_input, output) -> (new_reported_input, new_output)`
    ///
    /// A possible usage would be to swap the `output` and `reported_input`, so the `reported_input` may end up in
    /// a `Result::Ok` (instead of the usual output):
    /// ```nocompile
    ///     .map_reported_input_and_output(|reported_input, output| (output, reported_input) )
    pub fn map_reported_input_and_output<NewReportedInput,
                                         NewOutput,
                                         F: FnOnce(ReportedInput, Output) -> (NewReportedInput, NewOutput)>
                                        (self, f: F) -> ResolvedResult<NewReportedInput, OriginalInput, NewOutput, ErrorType> {
        match self {
            ResolvedResult::Fatal         { input, error}                                      => ResolvedResult::Fatal         { input, error },
            ResolvedResult::GivenUp       { input, retry_errors, fatal_error } => ResolvedResult::GivenUp       { input, retry_errors, fatal_error },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error } => ResolvedResult::Unrecoverable { input, retry_errors, fatal_error },

            ResolvedResult::Recovered     { reported_input, output, retry_errors } => {
                let (reported_input, output) = f(reported_input, output);
                ResolvedResult::Recovered { reported_input, output, retry_errors }
            },
            ResolvedResult::Ok { reported_input, output } => {
                let (reported_input, output) = f(reported_input, output);
                ResolvedResult::Ok { reported_input, output }
            },
        }
    }

    /// Allows changing the `input` and `errors` of already resolved & unsuccessful operations:
    ///   - fatal_map_fn(input, fatal_error) -> (new_input, new_fatal_error)
    ///   - retry_errors_map_fn(retry_error) -> new_retry_error
    /// 
    /// Covers the case where it is needed to move the `input` into the fatal `error`, so it may contain the
    /// failed payload when converted to `Result::Err`.
    /// 
    /// See also [RetryResult::map_inputs_and_errors()] for similar semantics when you wish to skip retrying.
    pub fn map_input_and_errors<NewOriginalInput,
                                 NewErrorType,
                                 FatalMapFn:       FnOnce(OriginalInput, ErrorType) -> (NewOriginalInput, NewErrorType),
                                 RetryErrorsMapFn: FnMut(ErrorType)                 -> NewErrorType>

                                (self,
                                 fatal_map_fn:        FatalMapFn,
                                 retry_errors_map_fn: RetryErrorsMapFn)

                                -> ResolvedResult<ReportedInput, NewOriginalInput, Output, NewErrorType> {

        match self {
            ResolvedResult::Ok            { reported_input, output }                               => ResolvedResult::Ok            { reported_input, output },
            ResolvedResult::Recovered     { reported_input, output, retry_errors } => ResolvedResult::Recovered     { reported_input, output, retry_errors: retry_errors.into_iter().map(retry_errors_map_fn).collect() },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }  => {
                let (new_input, new_fatal_error) = fatal_map_fn(input, fatal_error);
                ResolvedResult::Unrecoverable { input: new_input, retry_errors: retry_errors.into_iter().map(retry_errors_map_fn).collect(), fatal_error: new_fatal_error }
            },
            ResolvedResult::GivenUp { input, retry_errors, fatal_error } => {
                let (new_input, new_fatal_error) = fatal_map_fn(input, fatal_error);
                ResolvedResult::GivenUp { input: new_input, retry_errors: retry_errors.into_iter().map(retry_errors_map_fn).collect(), fatal_error: new_fatal_error }
            },
            ResolvedResult::Fatal { input, error } => {
                let (new_input, new_error) = fatal_map_fn(input, error);
                ResolvedResult::Fatal { input: new_input, error: new_error }
            },
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
From<ResolvedResult<ReportedInput,
                    OriginalInput,
                    Output,
                    ErrorType>> for
Result<Output, ErrorType> {

    /// Lossy operation that will downgrade this already resolved operation (after some possible retries) into a `Result<>`
    fn from(resolved_result: ResolvedResult<ReportedInput, OriginalInput, Output, ErrorType>) -> Result<Output, ErrorType> {
        match resolved_result {
            ResolvedResult::Ok { reported_input: _, output }                            => Ok(output),
            ResolvedResult::Fatal { input: _, error }                                => Err(error),
            ResolvedResult::Recovered { reported_input: _, output, retry_errors: _ }    => Ok(output),
            ResolvedResult::GivenUp { input: _, retry_errors: _, fatal_error }       => Err(fatal_error),
            ResolvedResult::Unrecoverable { input: _, retry_errors: _, fatal_error } => Err(fatal_error),
        }
    }
}


use std::{
    fmt::Debug,
    collections::BTreeMap,
};
/// builds an as-short-as-possible list of `retry_errors` occurrences (out of order),
/// provided `ErrorType` implements the `Debug` trait.
pub fn loggable_retry_errors<ErrorType: Debug>(retry_errors: &Vec<ErrorType>) -> String {
    let mut counts = BTreeMap::<String, u32>::new();
    for error in retry_errors {
        let error_string = format!("{:?}", error);
        *counts.entry(error_string).or_insert(0) += 1;
    }
    counts.into_iter()
        .map(|(error, count)| format!("{count}x: '{error}'"))
        .collect::<Vec<_>>()
        .join(", ")
}
