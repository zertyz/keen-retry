# keen-retry

A simple -- yet powerful -- zero-cost-abstractions & zero-copy lib for error handling & recovery -- with the following functionalities, focusing on performance and code expressiveness:
  * Provides an idiomatic, Rust-like, API by allowing operations to *return a retryable type* -- using a functional approach for expressing the retry logic in par with what we have for `Result<>` and `Option<>`. This eases *building pipelines of retryable operations immune to the callback-hell phenomenon*;
  * Zero-cost-abstractions: if the operation succeeds or fails fatably in the initial attempt, no extra code is run -- zero-cost if no retrying is necessary;
  * Distinct operations for the first (initial) attempt and the retrying ones, allowing the retryable operations to do extra (expensive) checks for detailed instrumentation, error reporting and, possibly, more elaborated (nested) retrying logics (such as reconnecting to a server if a channel detected a connection drop while sending a payload -- in this case, a failure in retrying a connection might either be transient or fatal: *the server might say it is full and that we should try another peer, or it might say the given credentials are not recognized*);
  * Full support for both `sync` and `async` operations.

# Components
  * Wrappers on retryable operations' return type -- similar (and conversible) to `Result<>`:
    - `RetryConsumerResult` -- for zero-copy consumers that consume the payload on success, returning them back on Error. Pattern examples: `crossbeam` & `reactive-mutiny` channels;
    - `ProducerRetryResult` -- for operations that take no input, but may return data;
    - `RetryResult`         -- for operations that consume the input on success (or fatal failure) and return data;
  * The `KeenRetry[Async]Executor`, responsible for executing the retry logic according to the chosen backoff algorithm and limits;
  * `ResolvedResult`, containing all possibilities for the retryable operations (conversible to `Result<>`): `Ok` (on the first attempt), `Fatal` (on the first attempt), `Recovered` (Ok on a retry attempt), `GavenUp` (exceeded retry attempts), `Unrecoverable` (Fatal on a retry attempt). Also contain some nice facilities for instrumentation (like building a succinct report of the retry errors, to ease logging)

# Taste it

First, the operation must adhere to the `keen-retry` API -- to distinguish whether retrying is necessary:

```nocompile
    async fn connect_to_server_retry(&self) -> RetryConsumerResult<(), (), ConnectionErrors> {
        self.connect_to_server_raw().await
            .map_or_else(|error| match error.is_fatal() {
                            true  => RetryConsumerResult::Fatal { input: (), error },
                            false => RetryConsumerResult::Retry { input: (), error },
                         },
                         |_| RetryConsumerResult::Ok { reported_input: (), output: () })
    }
```

Then, a simple retryable logic:

```nocompile
    pub async fn connect_to_server(self: &Arc<Self>) -> Result<(), ConnectionErrors> {
        let cloned_self = Arc::clone(&self);
        self.connect_to_server_retry().await
            .retry_with_async(|_| cloned_self.connect_to_server_retry())
            .with_delays((10..=130).step_by(10).map(|millis| Duration::from_millis(millis)))    // 13 attempts with an arithmetic progression on the sleeping time between attempts
            .await
            .inspect_recovered(|_, _, loggable_retry_errors, retry_errors_list| println!("## `connect_to_server()`: successfully connected after retrying {} times (failed attempts: [{loggable_retry_errors}])", retry_errors_list.len()))
            .into()
    }

```

For a fully fledged instrumentation, as seen in production applications, take a look at `tests/use_cases.rs`,
where all operations, errors, and nested levels of retries are demonstrated.