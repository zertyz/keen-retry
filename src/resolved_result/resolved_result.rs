//! Resting place for [ResolvedResult]


/// Contains all possibilities for finished retryable operations -- conversible to `Result<>` --
/// and some nice facilities for instrumentation (like building a succinct report of the retry errors)
pub enum ResolvedResult<OkResult,
                        RetryPayload,
                        ErrorType> {
    Ok {
        // TODO: should we be using `input` & `output` instead of `payload` this enum must become something like `ExecutedOperation`
        payload: OkResult,
    },

    Fatal {
        payload: Option<OkResult>,
        error:   ErrorType,
    },

    Recovered {
        payload: OkResult,
        retry_errors: Vec<ErrorType>,
    },

    GivenUp {
        payload:      RetryPayload,
        retry_errors: Vec<ErrorType>,
    },

    Unrecoverable {
        payload:      Option<RetryPayload>,
        retry_errors: Vec<ErrorType>,
        fatal_error:  ErrorType,
    },

}

impl<OkResult,
     RetryPayload,
     ErrorType>

ResolvedResult<OkResult,
               RetryPayload,
               ErrorType> {

    pub fn inspect_ok<IgnoredReturn,
                      F: FnOnce(&OkResult) -> IgnoredReturn>
                     (self, f: F) -> Self {
        if let Self::Ok { ref payload } = self {
            f(payload);
        }
        self
    }

    pub fn inspect_fatal<IgnoredReturn,
                         F: FnOnce(&Option<OkResult>, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Fatal { ref payload, ref error } = self {
            f(payload, error);
        }
        self
    }

    pub fn inspect_recovered<IgnoredReturn,
                             F: FnOnce(&OkResult, &Vec<ErrorType>) -> IgnoredReturn>
                            (self, f: F) -> Self {
        if let Self::Recovered { ref payload, ref retry_errors } = self {
            f(payload, retry_errors);
        }
        self
    }

    pub fn inspect_given_up<IgnoredReturn,
                            F: FnOnce(&RetryPayload, &Vec<ErrorType>) -> IgnoredReturn>
                           (self, f: F) -> Self {
        if let Self::GivenUp { ref payload, ref retry_errors } = self {
            f(payload, retry_errors);
        }
        self
    }

    pub fn inspect_unrecoverable<IgnoredReturn,
                                 F: FnOnce(&Option<RetryPayload>, &Vec<ErrorType>, &ErrorType) -> IgnoredReturn>
                                (self, f: F) -> Self {
        if let Self::Unrecoverable { ref payload, ref retry_errors, ref fatal_error } = self {
            f(payload, retry_errors, fatal_error);
        }
        self
    }

}

impl<OkResult,
     RetryPayload,
     ErrorType>
From<Result<OkResult, ErrorType>> for
ResolvedResult<OkResult,
               RetryPayload,
               ErrorType> {

    fn from(result: Result<OkResult, ErrorType>) -> Self {
        match result {
            Ok(payload) => ResolvedResult::Ok    { payload },
            Err(error)   => ResolvedResult::Fatal { payload: None, error }
        }
    }
}

impl<OriginalPayload,
     RetryPayload,
     ErrorType>
Into<Result<OriginalPayload, ErrorType>> for
ResolvedResult<OriginalPayload,
               RetryPayload,
               ErrorType> {

    fn into(self) -> Result<OriginalPayload, ErrorType> {
        match self {
            ResolvedResult::Ok { payload } => Ok(payload),
            ResolvedResult::Fatal { payload, error } => Err(error),
            ResolvedResult::Recovered { payload, retry_errors } => Ok(payload),
            ResolvedResult::GivenUp { payload, mut retry_errors } => Err(retry_errors.pop().unwrap()),
            ResolvedResult::Unrecoverable { payload, retry_errors, fatal_error } => Err(fatal_error),
        }
    }
}