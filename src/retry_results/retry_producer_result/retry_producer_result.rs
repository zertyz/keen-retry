//! Resting place for [RetryProducerResult]


use crate::RetryResult;

/// Wrapper for the return type of fallible & retryable functions -- a replacement for `Result<Output, ErrorType>`.
pub type RetryProducerResult<Output, ErrorType> = RetryResult<(), (), Output, ErrorType>;
