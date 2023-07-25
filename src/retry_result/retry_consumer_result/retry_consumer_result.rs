//! Resting place for [RetryConsumerResult]


use crate::KeenRetryExecutor;

/// Wrapper for the return type of fallible & retryable functions -- a replacement for `Result<OkPayload, ErrorType>`.\
/// Considering zero-copy, both `Retry` & `Fatal` variants will contain the original input payload -- which is consumed by an `Ok` operation.
pub enum RetryConsumerResult<OkResult,
                             RetryPayload,
                             ErrorType> {
    Ok {
        // here we considering payload=input, but to make this enum general, we must be able to specify both `input` and `output`
        payload: OkResult,
    },

    Retry {
        payload: RetryPayload,  // payload should become `input`
        error:   ErrorType,
    },

    Fatal {
        payload: RetryPayload,  // payload should become `input`
        error:   ErrorType,
    },
}

impl<OkResult,
     RetryPayload,
     ErrorType>
RetryConsumerResult<OkResult,
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

    pub fn inspect_retry<IgnoredReturn,
                         F: FnOnce(&RetryPayload, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Retry { ref payload, ref error } = self {
            f(payload, error);
        }
        self
    }

    pub fn inspect_fatal<IgnoredReturn,
                         F: FnOnce(&RetryPayload, &ErrorType) -> IgnoredReturn>
                        (self, f: F) -> Self {
        if let Self::Fatal { ref payload, ref error } = self {
            f(payload, error);
        }
        self
    }

    pub fn map_ok_result<NewOkResult,
                         F: FnOnce(OkResult) -> NewOkResult>
                        (self, f: F) -> RetryConsumerResult<NewOkResult, RetryPayload, ErrorType> {
        match self {
            RetryConsumerResult::Ok    { payload }                    => RetryConsumerResult::Ok    { payload: f(payload) },
            RetryConsumerResult::Retry { payload, error } => RetryConsumerResult::Retry { payload, error },
            RetryConsumerResult::Fatal { payload, error } => RetryConsumerResult::Fatal { payload, error }
        }
    }

    pub fn map_retry_payload<NewRetryPayload,
                             F: FnOnce(RetryPayload) -> NewRetryPayload>
                            (self, f: F) -> RetryConsumerResult<OkResult, NewRetryPayload, ErrorType> {
        match self {
            RetryConsumerResult::Ok    { payload }                     => RetryConsumerResult::Ok    { payload },
            RetryConsumerResult::Retry { payload, error }  => RetryConsumerResult::Retry { payload: f(payload), error },
            RetryConsumerResult::Fatal { payload, error }  => RetryConsumerResult::Fatal { payload: f(payload), error },
        }
    }

    pub fn map_retry_error<F: FnOnce(ErrorType) -> ErrorType>
                          (self, f: F) -> RetryConsumerResult<OkResult, RetryPayload, ErrorType> {
        match self {
            RetryConsumerResult::Ok    { payload }                     => RetryConsumerResult::Ok    { payload },
            RetryConsumerResult::Retry { payload, error }  => RetryConsumerResult::Retry { payload, error: f(error) },
            RetryConsumerResult::Fatal { payload, error }  => RetryConsumerResult::Fatal { payload, error },
        }
    }

    pub fn map_fatal_error<F: FnOnce(ErrorType) -> ErrorType>
                          (self, f: F) -> RetryConsumerResult<OkResult, RetryPayload, ErrorType> {
        match self {
            RetryConsumerResult::Ok    { payload }                    => RetryConsumerResult::Ok    { payload },
            RetryConsumerResult::Retry { payload, error } => RetryConsumerResult::Retry { payload, error },
            RetryConsumerResult::Fatal { payload, error } => RetryConsumerResult::Fatal { payload, error: f(error) },
        }
    }

    /// Upgrades this [RetryResult] into a [KeenRetryExecutor], which will, on its turn, be upgraded to [ResolvedResult], containing the final results of the retryable operation
    pub fn retry_with<RetryFn: FnMut(RetryPayload) -> RetryConsumerResult<OkResult, RetryPayload, ErrorType>>
                     (self, retry_operation: RetryFn)
                     -> KeenRetryExecutor<OkResult, RetryPayload, ErrorType, RetryFn> {

        match self {
            RetryConsumerResult::Ok    { payload }                    => KeenRetryExecutor::from_resolved(Ok(payload)),
            RetryConsumerResult::Fatal { payload, error } => KeenRetryExecutor::from_resolved(Err(error)),
            RetryConsumerResult::Retry { payload, error } => KeenRetryExecutor::new(payload, retry_operation, error),
        }
    }

}