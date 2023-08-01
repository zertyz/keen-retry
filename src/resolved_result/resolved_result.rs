//! Resting place for [ResolvedResult]


/// Contains all possibilities for finished retryable operations -- conversible to `Result<>` --
/// and some nice facilities for instrumentation (like building a succinct report of the retry errors)
pub enum ResolvedResult<ReportedInput,
                        OriginalInput,
                        Output,
                        ErrorType> {
    Ok {
        reported_input: ReportedInput,
        output:         Output,
    },

    Fatal {
        input: OriginalInput,
        error: ErrorType,
    },

    Recovered {
        reported_input: ReportedInput,
        output:         Output,
        retry_errors:   Vec<ErrorType>,
    },

    GivenUp {
        input:        OriginalInput,
        retry_errors: Vec<ErrorType>,
        fatal_error:  ErrorType,
    },

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

    pub fn from_ok_result(reported_input: ReportedInput, output: Output) -> Self {
        ResolvedResult::Ok { reported_input, output }
    }

    pub fn from_err_result(original_input: OriginalInput, error: ErrorType) -> Self {
        ResolvedResult::Fatal { input: original_input, error }
    }

    pub fn inspect_ok<IgnoredReturn,
                      F: FnOnce(&ReportedInput, &Output) -> IgnoredReturn>
                     (self, f: F) -> Self {
        if let Self::Ok { ref reported_input, ref output } = self {
            f(reported_input, output);
        }
        self
    }

    pub fn inspect_fatal<IgnoredReturn,
                         F: FnOnce(&OriginalInput, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Fatal { ref input, ref error } = self {
            f(input, error);
        }
        self
    }

    /// Spots on the data of an already resolved (and unsuccessful) operation that required some retry attempts to succeed.
    ///   - `f(&reported_input, &output, &retry_errors_list)`
    ///   - if the error type implements `Debug`, you can do `let loggable_retry_errors = keen_retry::loggable_retry_errors(&retry_errors_list)`
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
    pub fn inspect_unrecoverable<IgnoredReturn,
                                 F: FnOnce(&OriginalInput, &Vec<ErrorType>, /*fatal_error: */&ErrorType) -> IgnoredReturn>
                                (self, f: F) -> Self {
        if let Self::Unrecoverable { ref input, ref retry_errors, ref fatal_error } = self {
            f(input, retry_errors, fatal_error);
        }
        self
    }

    /// Changes the original input data of an already resolved (but unsuccessful) operation.
    ///   - `f(original_input) -> new_original_input`.
    ///
    /// A possible usage would be to downgrade a laden payload to its initial form, so it may be returned back to the caller:
    /// ```nocompile
    ///     // downgrades the laden payload that was useful when logging the retry attempts & outcomes
    ///     .map_unrecoverable_input(|(loggable_payload, original_payload, retry_start)| original_payload)
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

    /// Allows changing the reported input data of an already resolved (and unsuccessful) operation.\
    /// A successful operation is likely to consume the input and the `reported_input` is expected, if desirable,
    /// to be a lightweight representation of it -- lets say, for instrumentation purposes.\
    /// This method allows a laden `reported_input` to be downgraded to its original form, where:
    ///   - `f(reported_input) -> new_reported_input`.
    ///
    /// Example:
    /// ```nocompile
    ///     // downgrades the laden `reported_input` that was useful during logging & retrying
    ///     .map_reported_input(|(loggable_payload, duration)| loggable_payload)
    pub fn map_reported_input<NewReportedInput,
                              F: FnOnce(ReportedInput) -> NewReportedInput>
                             (self, f: F) -> ResolvedResult<NewReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            ResolvedResult::Ok            { reported_input, output }                              => ResolvedResult::Ok            { reported_input: f(reported_input), output },
            ResolvedResult::Fatal         { input, error}                                        => ResolvedResult::Fatal         { input, error },
            ResolvedResult::Recovered     { reported_input, output, retry_errors }  => ResolvedResult::Recovered     { reported_input: f(reported_input), output, retry_errors },
            ResolvedResult::GivenUp       { input, retry_errors, fatal_error }     => ResolvedResult::GivenUp       { input, retry_errors, fatal_error },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }     => ResolvedResult::Unrecoverable { input, retry_errors, fatal_error },
        }
    }

    /// Allows changing the `reported_input` and `output` of an already resolved (and successful) operation.\
    ///   - `f(reported_input, output) -> (new_reported_input, new_output)`
    ///
    /// A possible usage would be to swap the `output` and `reported_input`, so the `reported_input` may end up in
    /// a `Result<>` (instead of the usual output):
    /// ```nocompile
    ///     .map_reported_input_and_output(|reported_input, output| (output, reported_input) )
    pub fn map_reported_input_and_output<NewReportedInput,
                                         NewOutput,
                                         F: FnOnce(ReportedInput, Output) -> (NewReportedInput, NewOutput)>
                                        (self, f: F) -> ResolvedResult<NewReportedInput, OriginalInput, NewOutput, ErrorType> {
        match self {
            ResolvedResult::Fatal         { input, error}                                    => ResolvedResult::Fatal         { input, error },
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

    /// Allows changing the `input` and `errors` of an already resolved (but unsuccessful) operation.\
    /// A possible usage would be to (logically) move the `input` into the `errors`, so they may contain the
    /// failed payload -- keep in mind this method allows this to done in a zero-copy manner when compiled in Release Mode.
    /// This is needed as the `input`, which originally might have been into the `error`, was likely taken out of it for the
    /// retry process to be kept zero-copy.
    pub fn map_errors<NewOriginalInput,
                      NewErrorType:     Debug,
                      FatalErrorMapFn:  FnOnce(ErrorType, OriginalInput) -> (NewOriginalInput, NewErrorType),
                      RetryErrorsMapFn: FnMut(ErrorType)                 -> NewErrorType>

                     (self,
                      fatal_error_map:  FatalErrorMapFn,
                      retry_errors_map: RetryErrorsMapFn)

                      -> ResolvedResult<ReportedInput, NewOriginalInput, Output, NewErrorType> {

        match self {
            ResolvedResult::Ok            { reported_input, output }                               => ResolvedResult::Ok            { reported_input, output },
            ResolvedResult::Recovered     { reported_input, output, retry_errors } => ResolvedResult::Recovered     { reported_input, output, retry_errors: retry_errors.into_iter().map(retry_errors_map).collect() },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }  => {
                let (new_input, new_fatal_error) = fatal_error_map(fatal_error, input);
                ResolvedResult::Unrecoverable { input: new_input, retry_errors: retry_errors.into_iter().map(retry_errors_map).collect(), fatal_error: new_fatal_error }
            },
            ResolvedResult::GivenUp { input, retry_errors, fatal_error } => {
                let (new_input, new_fatal_error) = fatal_error_map(fatal_error, input);
                ResolvedResult::GivenUp { input: new_input, retry_errors: retry_errors.into_iter().map(retry_errors_map).collect(), fatal_error: new_fatal_error }
            },
            ResolvedResult::Fatal { input, error } => {
                let (new_input, new_error) = fatal_error_map(error, input);
                ResolvedResult::Fatal { input: new_input, error: new_error }
            },
        }
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

    /// Lossy operation that will convert this already resolved operation (after some possible retries) into a `Result<>`
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
