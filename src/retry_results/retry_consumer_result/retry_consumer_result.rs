//! Resting place for [RetryConsumerResult]


use crate::RetryResult;

/// Wrapper for the return type of fallible & retryable functions -- a replacement for `Result<Output, ErrorType>`.\
/// Considering zero-copy, both `Retry` & `Fatal` variants will contain the original input payload -- which is consumed by an `Ok` operation.
pub type RetryConsumerResult<ReportedInput, OriginalInput, ErrorType> = RetryResult<ReportedInput, OriginalInput, (), ErrorType>;
