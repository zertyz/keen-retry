# keen-retry

Built from where the `retry` crate stopped, adds in the following functionalities, focusing on performance and code expressiveness:
  * Provides an alternative, more idiomatic Rust-like API by allowing operations to *return a retryable type* -- using a functional approach for expressing the retry logic in par with what we have for `Result<>` and `Option<>`. This eases *building pipelines of retryable operations immune to the callback-hell phenomenon* -- see the "Taste it" section;
  * Improves the performance of `retry` by introducing zero-cost-abstractions if the operation succeeds or fails fatably in the initial attempt: no extra code is run (compared to not using this lib) -- zero-cost if no retrying is neccessary;
  * Distinct operations for the first (initial) attempt and the retrying ones, allowing the retryable operations to do extra (expensive) checks for detailed instrumentation, error reporting and, possibly, more elaborated retrying logics (such as reconnecting to a server if a channel detected a connection drop while sending a payload -- in this case, a failure in retrying a connection might either be transient or fatal... the server may say it is full and that we should try another peer or it may say the given credentials are not recognized);
  * Allows retrying `async` operations

# Components
  * Wrappers on retryable operations' return type -- similar (and conversible) to `Result<>`:
    - `RetryConsumerResult` -- for zero-copy consumers that consume the payload on success, returning them back on Error. Pattern examples: `crossbeam` & `reactive-mutiny` channels;
    - `ProducerRetryResult` -- for general patterns that wouldn't benefit of the internal zero-copy mechanisms. Suitable for producers, syncing and other routines with side-effects;
  * The `KeenRetryExecutor`, responsible for executing the retry logic according to the chosen backoff algorithm and limits, keeping track of retry metrics;
  * `ResolvedResult`, containing all possibilities for the retryable operations (conversible to `Result<>`): OK (on the first attempt), Fatal (on the first attempt), Recovered (OK on a retry attempt), GavenUp (exceeded retry attempts), Unrecoverable (Fatal on a retry attempt). Also contain some nice facilities for instrumentation (like building a succinct report of the retry errors)

# Taste it

```nocompile

    fn send::<T>::(socket: &Socket, payload: T) -> RetryConsumerResult<ErrorType> {
        if channel.is_full() {
            RetryConsumerResult::Retryable(Box::from(format!("Channel is full")), payload)
        } else {
            channel.send(T)
                .map_ok(|_| RetryConsumerResult::Ok(()))
                .map_err(|err| RetryConsumerResult::Fatal(err, payload))
        }
    }

    fn send_retry(socket: &mut Socket, payload: &str) -> Result<(), ErrorType> {
        send(payload)
            .[map_|inspect_]ok()    // in case the operation succeeded on the first shot
            .[map_|inspect_]fatal(|fatal_err| error!("`send()`: fatal error (won't retry): {:?}", err))
            .map_retry_payload(|payload| (payload, format!("{:?}", payload))
            .inspect_retry_payload(|&(payload, loggable_payload)| {})
            .retry_with(|(payload, loggable_payload)| {
                // verify the connection
                if !socket.is_connected() {
                    socket = connect_to_server_retry()
                        .map_err(|err| Box::from(format!("Send failed (gave up retrying) due to not being able to reconnect to the server due to a fatal error: {}", err)))?;   // `connect_to_server()` itself implement a similar retrying mechanism. map_err is used to better error messages
                }
                send(payload)
                    .map_ok(loggable_payload)
            })
            // the above statement returns a KeenRetryExecutor<RetryResult<_>>, which is executed by one of its methods
            .with_delays(delays)|.spinning_forever()|.yielding_forever()|.spinning_at_most()|.yielding_at_most()
            // from this point down, we have a KeenRetryResult object -- a resolved RetryResult in which retrying is no longer possible... just for fetching the values
            .[map_|inspect]_unrecoverable(|retry_errors_list, (payload, _)| error!("`send()` has exhausted all {} retrying attempts", retryable_errors_list.len()))
            .[map_|inspect_]retry_fatal(|retry_errors_list, fatal_err, (payload, _)| error!("`send()`: fatal error after retrying {} time(s): {:?}", retry_errors_list.len()+1, err))
            .[map_|inspect_]retry_ok(|retry_errors_list, loggable_payload| trace!("send(): successfully sent {}", loggable_payload))
            //.to_result()  no need, as this fella implements Into<Result<O,E>>
    }
```

```nocpmpile
fn connect_to_database() -> KeenRetry<Connection, RetryableError, FatalError> { ... }
fn query(connection: &mut Connection, command) -> KeenRetry<Data, RetryableError, FatalError> { ... }
fn connect_to_server() -> KeenRetry<Socket, RetryableError, FatalError> { ... }
fn send_event(socket: &mut Socket, data: &Data) -> KeenRetry<(), RetryableError, FatalError> {
    KeenRetry::new(
        // the first operation
        || socket.send(data)?,
        // the retry operation -- may reconnect if needed, which may fail fatably if `Error::InvalidCredentials`
        || {
            if socket.is_not_open() {
                log!("Restablishing the Connection...")
                socket.reopen_connection()
                    .map_err(|err| {
                      if let Error::InvalidCredentials = err {
                        FatalError::from(err)
                      }
                    })?;
            }
            socket.send(data)?
        }
    )
}

// `KeenRetry` receives an `operation()` function to retry (it may either return an "OK result", a "retryable error" or a "fatal error" -- in which case the KeenRetry::retry_with_delay() method would stop.
// A "retryable error" can also be reported as a "fatal error" (conversible into a regular Rust `Result::Err`) if the number of retry attempts had exceeded.

const RETRY_ATTEMPTS = 10;
const DELAY = Fixed::from_millis(100);  // from the `retry` crate

fn business_logic() -> Result<(), Error> {
let connection = connect_to_database()
    .or_else(|keen_retry| keen_retry.retry_with_delay(RETRY_ATTEMPTS, DELAY))?;             // .retry() runs code provided by the function called above
let data = query(connection, "select all that I want")
    .or_else(|keen_retry| keen_retry.retry_with_delay(RETRY_ATTEMPTS, DELAY))?;
send_event(data)
    .or_else(|keen_retry| keen_retry.retry_with_delay(RETRY_ATTEMPTS, DELAY))?;
}

// notice that the business logic might also be implemented without calling `.or_else()` or without caring for `KeenRetry` -- its error return type would still be converted to a regular Rust Error

```
