//! Resting place for [ResolvedResult]


/// Contains all possibilities for finished retryable operations -- conversible to `Result<>` --
/// and some nice facilities for instrumentation (like building a succinct report of the retry errors)
pub enum ResolvedResult<ReportedInput,
                        OriginalInput,
                        Output,
                        ErrorType> {
    Ok {
        reported_input: Option<ReportedInput>,
        output:         Output,
    },

    Fatal {
        reported_input: Option<ReportedInput>,
        error:          ErrorType,
    },

    Recovered {
        reported_input: ReportedInput,
        output:         Output,
        retry_errors:   Vec<ErrorType>,
    },

    GivenUp {
        input:        OriginalInput,
        retry_errors: Vec<ErrorType>,
    },

    Unrecoverable {
        input:        Option<OriginalInput>,
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

    pub fn inspect_ok<IgnoredReturn,
                      F: FnOnce(&Option<ReportedInput>, &Output) -> IgnoredReturn>
                     (self, f: F) -> Self {
        if let Self::Ok { ref reported_input, ref output } = self {
            f(reported_input, output);
        }
        self
    }

    pub fn inspect_fatal<IgnoredReturn,
                         F: FnOnce(&Option<ReportedInput>, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Fatal { reported_input: ref input, ref error } = self {
            f(input, error);
        }
        self
    }

    pub fn inspect_recovered<IgnoredReturn,
                             F: FnOnce(&ReportedInput, &Output, &Vec<ErrorType>) -> IgnoredReturn>
                            (self, f: F) -> Self {
        if let Self::Recovered { ref reported_input, ref output, ref retry_errors } = self {
            f(reported_input, output, retry_errors);
        }
        self
    }

    pub fn inspect_given_up<IgnoredReturn,
                            F: FnOnce(&OriginalInput, &Vec<ErrorType>) -> IgnoredReturn>
                           (self, f: F) -> Self {
        if let Self::GivenUp { ref input, ref retry_errors } = self {
            f(input, retry_errors);
        }
        self
    }

    pub fn inspect_unrecoverable<IgnoredReturn,
                                 F: FnOnce(&Option<OriginalInput>, &Vec<ErrorType>, &ErrorType) -> IgnoredReturn>
                                (self, f: F) -> Self {
        if let Self::Unrecoverable { ref input, ref retry_errors, ref fatal_error } = self {
            f(input, retry_errors, fatal_error);
        }
        self
    }

    pub fn map_retry_payload<NewOriginalInput,
                             F: FnOnce(OriginalInput) -> NewOriginalInput>
                            (self, f: F) -> ResolvedResult<ReportedInput, NewOriginalInput, Output, ErrorType> {
        match self {
            ResolvedResult::Ok            { reported_input, output }                                             => ResolvedResult::Ok            { reported_input, output },
            ResolvedResult::Fatal         { reported_input: input, error }                                                     => ResolvedResult::Fatal         { reported_input: input, error },
            ResolvedResult::Recovered     { reported_input, output, retry_errors }                       => ResolvedResult::Recovered     { reported_input, output, retry_errors },
            ResolvedResult::GivenUp       { input, retry_errors }                                                 => ResolvedResult::GivenUp       { input: f(input), retry_errors },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error } if input.is_some() => ResolvedResult::Unrecoverable { input: Some(f(input.unwrap())), retry_errors, fatal_error },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }                    => ResolvedResult::Unrecoverable { input: None,                    retry_errors, fatal_error },
        }
    }

    pub fn map_reported_input<NewReportedInput,
                              F: FnOnce(Option<ReportedInput>) -> Option<NewReportedInput>>
                             (self, f: F) -> ResolvedResult<NewReportedInput, OriginalInput, Output, ErrorType> {
        match self {
            ResolvedResult::Ok            { reported_input, output }                              => ResolvedResult::Ok            { reported_input: f(reported_input), output },
            ResolvedResult::Fatal         { reported_input, error } if reported_input.is_some() => ResolvedResult::Fatal         { reported_input: f(reported_input), error },
            ResolvedResult::Fatal         { reported_input: _, error}                                              => ResolvedResult::Fatal         { reported_input: None, error },
            ResolvedResult::Recovered     { reported_input, output, retry_errors }        => ResolvedResult::Recovered     { reported_input: f(Some(reported_input)).unwrap(), output, retry_errors },
            ResolvedResult::GivenUp       { input, retry_errors }                                  => ResolvedResult::GivenUp       { input, retry_errors },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }     => ResolvedResult::Unrecoverable { input, retry_errors, fatal_error },
        }
    }

    pub fn map_reported_input_and_output<NewReportedInput,
                                         NewOutput,
                                         F: FnOnce(Option<ReportedInput>, Option<Output>) -> (Option<NewReportedInput>, NewOutput)>
                                        (self, f: F) -> ResolvedResult<NewReportedInput, OriginalInput, NewOutput, ErrorType> {
        match self {
            ResolvedResult::Fatal         { reported_input, error } if reported_input.is_some() => ResolvedResult::Fatal         { reported_input: f(reported_input, None).0, error },
            ResolvedResult::Fatal         { reported_input: _, error}                                              => ResolvedResult::Fatal         { reported_input: None, error },
            ResolvedResult::GivenUp       { input, retry_errors }                                  => ResolvedResult::GivenUp       { input, retry_errors },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error }     => ResolvedResult::Unrecoverable { input, retry_errors, fatal_error },

            ResolvedResult::Recovered     { reported_input, output, retry_errors } => {
                let (reported_input, output) = f(Some(reported_input), Some(output));
                ResolvedResult::Recovered { reported_input: reported_input.unwrap(), output, retry_errors }
            },
            ResolvedResult::Ok { reported_input, output } => {
                let (reported_input, output) = f(reported_input, Some(output));
                ResolvedResult::Ok { reported_input, output }
            },
        }
    }

    pub fn map_errors<NewErrorType,
                      FatalErrorMapFn:  FnOnce(ErrorType, Option<OriginalInput>) -> NewErrorType,
                      RetryErrorsMapFn: FnMut(ErrorType)                         -> NewErrorType>

                     (self,
                      fatal_error_map:      FatalErrorMapFn,
                      mut retry_errors_map: RetryErrorsMapFn)

                      -> ResolvedResult<ReportedInput, OriginalInput, Output, NewErrorType> {

        match self {
            ResolvedResult::Ok            { reported_input, output }                          => ResolvedResult::Ok            { reported_input, output },
            ResolvedResult::Fatal         { reported_input, error }                         => ResolvedResult::Fatal         { reported_input, error: fatal_error_map(error, None) },
            ResolvedResult::Recovered     { reported_input, output, retry_errors }    => ResolvedResult::Recovered     { reported_input, output, retry_errors: retry_errors.into_iter().map(|e| retry_errors_map(e)).collect() },
            ResolvedResult::GivenUp       { input, retry_errors }                              => ResolvedResult::GivenUp       { input, retry_errors: retry_errors.into_iter().map(|e| retry_errors_map(e)).collect() },
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error } => ResolvedResult::Unrecoverable { input: None, retry_errors: retry_errors.into_iter().map(|e| retry_errors_map(e)).collect(), fatal_error: fatal_error_map(fatal_error, input) },
        }
    }

}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType>
From<Result<Output, ErrorType>> for
ResolvedResult<ReportedInput,
               OriginalInput,
               Output,
               ErrorType> {

    fn from(result: Result<Output, ErrorType>) -> Self {
        match result {
            Ok(output)   => ResolvedResult::Ok    { reported_input: None, output },
            Err(error) => ResolvedResult::Fatal { reported_input: None, error }
        }
    }
}

impl<ReportedInput,
     OriginalInput,
     Output,
     ErrorType>
Into<Result<Output, ErrorType>> for
ResolvedResult<ReportedInput,
               OriginalInput,
               Output,
               ErrorType> {

    fn into(self) -> Result<Output, ErrorType> {
        match self {
            ResolvedResult::Ok { reported_input, output }                                           => Ok(output),
            ResolvedResult::Fatal { reported_input: input, error }                                          => Err(error),
            ResolvedResult::Recovered { reported_input, output, retry_errors: _ }                   => Ok(output),
            ResolvedResult::GivenUp { input, mut retry_errors }                                => Err(retry_errors.pop().unwrap()),
            ResolvedResult::Unrecoverable { input, retry_errors, fatal_error } => Err(fatal_error),
        }
    }
}