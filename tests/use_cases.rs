//! Demonstrates how the `keen-retry` library can be integrated with the Rust's original `Result<O,E>` API.
//! 
//! Shows all possible use cases in which retries could be useful (both sync & async):
//!   - using all the zero-cost-abstraction for retry instrumentation features
//!   - using only the retry logic (no instrumentation)
//!   - not using any retry at all (like when first integrating an existing code base into [keen-retry]).
//! 
//! For all cases, we test if the operation:
//!   1) is good at the first shot;
//!   2) failed fatably at the first shot;
//!   3) recovered from failure after retrying;
//!   4) gave up retrying due to exceeding the max attempts without success;
//!   5) failed fatably while retrying.

use keen_retry::{RetryConsumerResult, RetryProducerResult};
use std::{
    fmt::Debug,
    time::Duration,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32};
use std::sync::atomic::Ordering::Relaxed;
use std::time::SystemTime;
use thiserror::Error;


type StdErrorType = Box<dyn std::error::Error + Send + Sync>;

/// Uses our fake [Socket] connection, implementing a simple retrying mechanism (no instrumentations),
/// to test all possible scenarios: 
#[tokio::test]
async fn simple_retries() -> Result<(), StdErrorType> {

    let case_name = "1) Good at the first shot";
    // println!("\n{}:", case_name);
    let mut socket = Socket::new(13, 1, 2, 0, 0, 0);
    let result = socket.connect_to_server().await;
    assert_eq!(result, Ok(()), "In '{}'", case_name);
    assert_eq!(socket.is_connected(), true, "In '{}'", case_name);

    let case_name = "2) Failed fatably at the first shot";
    // println!("\n{}:", case_name);
    let mut socket = Socket::new(13, 2, 1, 0, 0, 0);
    let result = socket.connect_to_server().await;
    assert_eq!(result, Err(ConnectionErrors::WrongCredentials), "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "3) Recovered from failure on the 10th attempt";
    // println!("\n{}:", case_name);
    let mut socket = Socket::new(13, 10, 11, 0, 0, 0);
    let result = socket.connect_to_server().await;
    assert_eq!(result, Ok(()), "In '{}'", case_name);
    assert_eq!(socket.is_connected(), true, "In '{}'", case_name);

    let case_name = "4) Failed due to give up retrying after exceeding 13 attempts";
    // println!("\n{}:", case_name);
    let mut socket = Socket::new(13, 15, 16, 0, 0, 0);
    let result = socket.connect_to_server().await;
    assert_eq!(result, Err(ConnectionErrors::ServerTooBusy), "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "5) Failed fatably during the 10th retry attempt";
    // println!("\n{}:", case_name);
    let mut socket = Socket::new(13, 11, 10, 0, 0, 0);
    let result = socket.connect_to_server().await;
    assert_eq!(result, Err(ConnectionErrors::WrongCredentials), "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    Ok(())
}

/// Uses our fake [Socket] sending to show how to make a fully-fledged instrumentation, if retrying is necessary,
/// without incurring into any extra code if no retrying takes place (in which case, no instrumentation is done)
#[tokio::test]
async fn zero_cost_abstractions() -> Result<(), StdErrorType> {

    let case_name = "1) Good at the first shot";
    println!("\n{}:", case_name);
    let expected = Ok(format!("{:?}", MyPayload { message: case_name }));
    let mut socket = Socket::new(13, 1, 11, 13, 1, 2);
    socket.connect_to_server().await?;
    let result = socket.send(MyPayload { message: case_name }).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), true, "In '{}'", case_name);

    let case_name = "2) Failed fatably at the first shot";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::QuotaExhausted { payload: Some(to_send()), root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 0, 11, 13, 10, 1);
    socket.connect_to_server().await?;
    let result = socket.send(to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "3.1) Recovered from failure on the 10th attempt (connection was initially OK)";
    println!("\n{}:", case_name);
    let expected = Ok(format!("{:?}", MyPayload { message: case_name }));
    let mut socket = Socket::new(13, 0, 999, 13, 10, 11);
    socket.connect_to_server().await?;
    let result = socket.send(MyPayload { message: case_name }).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), true, "In '{}'", case_name);

    let case_name = "3.2) Recovered from failure on the 10th attempt (connection was initially NOT OK and will only succeed at the 5th attempt)";
    println!("\n{}:", case_name);
    let expected = Ok(format!("{:?}", MyPayload { message: case_name }));
    let mut socket = Socket::new(13, 5, 999, 13, 10, 11);
    let result = socket.send(MyPayload { message: case_name }).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), true, "In '{}'", case_name);

    let case_name = "4) Failed due to give up retrying `send()` after exceeding 13 attempts";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::ConnectionDropped { payload: Some(to_send()), root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 1, 999, 13, 15, 16);
    socket.connect_to_server().await?;
    let result = socket.send(to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "5.1) Failed fatably during the 10th retry attempt (in `send()`)";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::QuotaExhausted { payload: Some(to_send()), root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 1, 999, 13, 999, 10);
    socket.connect_to_server().await?;
    let result = socket.send(to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "5.2) Failed fatably during a retry attempt (on reconnection)";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::CannotReconnect { payload: Some(to_send()), root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 999, 999, 13, 15, 16);
    let result = socket.send(to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    Ok(())
}

/// Demonstrates that we may not care if a function uses the `keen-retry` API... it will still behave like `Result<>`
#[tokio::test]
async fn optable_api() -> Result<(), StdErrorType> {
    let case_name = "1) Ok operation";
    println!("\n{}:", case_name);
    let mut socket = Socket::new(13, 1, 11, 13, 1, 2);
    socket.connect_to_server().await?;
    assert!(socket.receive().is_ok(), "Operation errored when it shouldn't at {case_name}");

    let case_name = "2) Err operation";
    println!("\n{}:", case_name);
    let mut socket = Socket::new(13, 1, 11, 13, 1, 2);
    assert!(socket.receive().is_err(), "Operation didn't errored as it should at {case_name}");

    Ok(())
}


/// Example of how to implement a retry mechanism free of instrumentation:
/// In this hypothetical scenario, when connecting, many retryable errors happen until
/// either a `success_latch_counter` or `fatal_failure_latch_counter` counts down to zero.
struct Socket {
    is_connected:                             AtomicBool,
    connection_max_retries:                   AtomicU32,
    /// number of calls for a successful connection to happen (if no failures as scheduled to happen after that, all further connection attempts will also be successful)
    connection_success_latch_countdown:       AtomicI32,
    connection_fatal_failure_latch_countdown: AtomicI32,
    connection_attempts:                      AtomicU32,
    sending_max_retries:                      AtomicI32,
    sending_success_latch_countdown:          AtomicI32,
    sending_fatal_failure_latch_countdown:    AtomicI32,
    sending_attempts:                         AtomicU32,
}

impl Socket {

    pub fn new(connection_max_retries:                   u32,
               connection_success_latch_countdown:       i32,
               connection_fatal_failure_latch_countdown: i32,
               sending_max_retries:                      i32,
               sending_success_latch_countdown:          i32,
               sending_fatal_failure_latch_countdown:    i32) -> Arc<Self> {
        Arc::new(Self {
            is_connected:                             AtomicBool::new(false),
            connection_max_retries:                   AtomicU32::new(connection_max_retries),
            connection_success_latch_countdown:       AtomicI32::new(connection_success_latch_countdown),
            connection_fatal_failure_latch_countdown: AtomicI32::new(connection_fatal_failure_latch_countdown),
            connection_attempts:                      AtomicU32::new(0),
            sending_max_retries:                      AtomicI32::new(sending_max_retries),
            sending_success_latch_countdown:          AtomicI32::new(sending_success_latch_countdown),
            sending_fatal_failure_latch_countdown:    AtomicI32::new(sending_fatal_failure_latch_countdown),
            sending_attempts:                         AtomicU32::new(0),
         })
    }

    pub async fn connect_to_server(self: &Arc<Self>) -> Result<(), ConnectionErrors> {
        let cloned_self = Arc::clone(&self);
        self.connect_to_server_retry().await
            .retry_with_async(|_| cloned_self.connect_to_server_retry())
            .with_delays((10..=130).step_by(10).map(|millis| Duration::from_millis(millis)))
            .await
            .inspect_recovered(|_, _, loggable_retry_errors, retry_errors_list| println!("## `connect_to_server()`: successfully connected after retrying {} times (failed attempts: [{loggable_retry_errors}])", retry_errors_list.len()))
            .into()
    }

    /// Shows off a rather "complex" retrying & instrumentation logic, but still zero-cost when not retrying:
    pub async fn send<T: Debug + PartialEq>

                     (self: &Arc<Self>,
                      payload: T)

                     -> Result<String, TransportErrors<T>> {

        let loggable_payload = format!("{:?}", payload);
        self.send_retry(payload)
            .inspect_fatal(|payload, fatal_err| println!("## `send({:?})`: fatal error (won't retry): {:?}", payload, fatal_err))
            .map_ok(|_, _| ((loggable_payload, Duration::ZERO), () ) )
            .map_input(|payload| ( /*loggable_payload:*/format!("{:?}", &payload), payload, SystemTime::now() ) )
            .retry_with_async(|(loggable_payload, payload, retry_start)| async move {
                if !self.is_connected() {
                    if let Err(err) = self.connect_to_server().await {
                        println!("## `send({loggable_payload})`: Error attempting to reconnect: {}", err);
                        return RetryConsumerResult::Fatal { input: (loggable_payload, payload, retry_start), error: TransportErrors::CannotReconnect {payload: None, root_cause: err.into()} };
                    }
                }
                self.send_retry(payload)
                    // notice the retry operation must map to the same types as the original operation:
                    .map_ok(|_, _| ((loggable_payload.clone(), retry_start.elapsed().unwrap()), () ))
                    .map_input(|payload| (loggable_payload, payload, retry_start) )
            })
            .with_delays((1..=13).map(|millis| Duration::from_millis((millis as f64 * 1.289f64.powi(millis)) as u64)))
            .await
            .inspect_given_up(|(loggable_payload, payload, retry_start), loggable_retry_errors, retry_errors_list, fatal_error| println!("## `send({:?})` FAILED after exhausting all {} retrying attempts in {:?} [{loggable_retry_errors}]",  payload, retry_errors_list.len(), retry_start.elapsed()))
            .inspect_unrecoverable(|(_loggable_payload, payload, retry_start), loggable_retry_errors, retry_errors_list, fatal_error| {
                println!("## `send({:?})`: fatal error after trying {} time(s) in {:?}: {:?} -- prior to that fatal failure, these retry attempts also failed: [{loggable_retry_errors}]", payload, retry_errors_list.len()+1, retry_start.elapsed().unwrap(), fatal_error);
            })
            .inspect_recovered(|(loggable_payload, duration), _output, loggable_retry_errors, retry_errors_list| println!("## `send({loggable_payload})`: succeeded after trying {} time(s) in {:?}: [{loggable_retry_errors}]", retry_errors_list.len()+1, duration))
            // remaps the input types back to their originals, simulating we are interested in only part of it (that other part was usefull only for instrumentation)
            .map_unrecoverable_input(|(loggable_payload, payload, retry_start)| payload)
            .map_reported_input(|(loggable_payload, duration)| loggable_payload)
            // demonstrates how to map the reported input (`loggable_payload` in our case) into the output, so it will be available at final `Ok(loggable_payload)` result
            .map_reported_input_and_output(|reported_input, output| (output, reported_input) )
            // go an extra mile to demonstrate how to place back the payloads of failed requests in `Option<>` field
            .map_errors(|mut fatal_error, payload| {
                            fatal_error.for_payload(|mut_payload| mut_payload.replace(payload));
                            ((), fatal_error)
                        },
                        |ignored| ignored)
            .into()
    }

    /// Proves the `keen-retry` API is optable... you may not recruit any of its features
    /// (in this case, the behavior will be identical to `Result<>`)
    pub fn receive(&self) -> Result<&'static str, TransportErrors<&'static str>> {
        self.receive_retry()
           .into()     // actually, this is the only needed part to convert between the `keen-retry` API and `Result<>` -- but notice there is no `.retry_with()` here
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Relaxed)
    }


    // low level functions //
    /////////////////////////

    async fn connect_to_server_retry(&self) -> RetryConsumerResult<(), (), ConnectionErrors> {
        self.connect_to_server_raw().await
            .map_or_else(|error| match error.is_fatal() {
                            true  => RetryConsumerResult::Fatal { input: (), error },
                            false => RetryConsumerResult::Retry { input: (), error },
                         },
                         |_| RetryConsumerResult::Ok { reported_input: (), output: () })
    }

    /// Simulates a real connection, failing according to the configuration
    async fn connect_to_server_raw(&self) -> Result<(), ConnectionErrors> {
        self.connection_attempts.fetch_add(1, Relaxed);
        if self.connection_success_latch_countdown.fetch_sub(1, Relaxed) <= 1 {
            self.connection_fatal_failure_latch_countdown.fetch_sub(1, Relaxed);
            self.is_connected.store(true, Relaxed);
            Ok(())
        } else if self.connection_fatal_failure_latch_countdown.fetch_sub(1, Relaxed) <= 1 {
            self.is_connected.store(false, Relaxed);
            Err(ConnectionErrors::WrongCredentials)
        } else if self.connection_success_latch_countdown.load(Relaxed) <= 0 {
            self.is_connected.store(true, Relaxed);
            Ok(())
        } else {
            self.is_connected.store(false, Relaxed);
            Err(ConnectionErrors::ServerTooBusy)
        }
    }

    fn send_retry<T: Debug + PartialEq>
                 (&self,
                  payload: T)
                 -> RetryConsumerResult<(), T, TransportErrors<T>> {

        self.send_raw(payload)
            .map_or_else(|error| match error.is_fatal() {
                             true  => {
                                 let (payload, error) = error.split();
                                 RetryConsumerResult::Fatal { input: payload, error }
                             },
                             false => {
                                 let (payload, error) = error.split();
                                 RetryConsumerResult::Retry { input: payload, error }
                             },
                         },
                         |_| RetryConsumerResult::Ok { reported_input: (), output: () })
    }

    fn send_raw<T: Debug + PartialEq>
               (&self,
                payload: T)
               -> Result<(), TransportErrors<T>> {

        if !self.is_connected.load(Relaxed) {
            Err(TransportErrors::NotConnected { payload: Some(payload) })
        } else {
            self.sending_attempts.fetch_add(1, Relaxed);
            if self.sending_success_latch_countdown.fetch_sub(1, Relaxed) <= 1 {
                self.sending_fatal_failure_latch_countdown.fetch_sub(1, Relaxed);
                Ok(())
            } else if self.sending_fatal_failure_latch_countdown.fetch_sub(1, Relaxed) <= 1 {
                self.is_connected.store(false, Relaxed);
                Err(TransportErrors::QuotaExhausted { payload: Some(payload), root_cause: format!("You abused sending. Please don't try to send anything else today or you will be banned!").into() })
            } else {
                self.is_connected.store(false, Relaxed);
                Err(TransportErrors::ConnectionDropped { payload: Some(payload), root_cause: format!("Socket read error (-1)").into() })
            }
        }
    }

    fn receive_retry(&self)
                    -> RetryProducerResult<&'static str, TransportErrors<&'static str>> {

        self.receive_raw()
            .map_or_else(|error| match error.is_fatal() {
                             true  => RetryProducerResult::Fatal { input: (), error },
                             false => RetryProducerResult::Retry { input: (), error },
                         },
                         |payload| RetryProducerResult::Ok { reported_input: (), output: payload })
    }

    /// This one doesn't fail extensively... all possible retries & recoveries are already tested in connect() & send().\
    /// Here, we are just interested into exploring the API over a function that produces results, instead of consuming it (like send() does)
    fn receive_raw(&self)
                  -> Result<&'static str, TransportErrors<&'static str>> {
        if !self.is_connected() {
            Err(TransportErrors::NotConnected { payload: None })
        } else {
            Ok("'Hello, Earthling!'")
        }
    }
}


/// Our test payload, used in `send()` operations
#[derive(Debug, PartialEq)]
struct MyPayload {
    message: &'static str,
}

/// Custom error types when connecting.\
/// Custom error types goes hand-in-hand with the `keen-retry` library (to distinguish among fatal and retryable errors).
/// Consider using the `thiserror` crate if you prefer to log custom error messages instead of the error name.
#[derive(Error, Debug, PartialEq)]
enum ConnectionErrors {
    /// A fatal error
    #[error("Wrong Credentials.")]
    WrongCredentials,
    /// A retryable error (becomes fatal if the number of retry attempts gets exhausted)
    #[error("Server too busy. Try again later.")]
    ServerTooBusy,
}
impl ConnectionErrors {
    pub fn is_fatal(&self) -> bool {
        match self {
            ConnectionErrors::WrongCredentials => true,
            ConnectionErrors::ServerTooBusy    => false,
        }
    }
}

/// Custom error type when sending/receiving.\
/// Here you'll see how to work with errors that carry the `payload` of failed consumer operations
/// -- the payload is wrapped in an `Option<>` as it needs to be owned, back and forth, by the `keen-retry` logic.
#[derive(Error, Debug)]
enum TransportErrors<Payload: Debug + PartialEq> {
    /// A fatal error (while sending)
    #[error("The sending quota has exhausted for the day.")]
    QuotaExhausted { payload: Option<Payload>, root_cause: StdErrorType },
    /// A retryable error (becomes fatal if the number of retry attempts gets exhausted)
    #[error("The connection was dropped.")]
    ConnectionDropped { payload: Option<Payload>, root_cause: StdErrorType },
    /// A fatal error (while retrying a failed reconnection)
    #[error("A reconnection attempt failed fatably.")]
    CannotReconnect { payload: Option<Payload>, root_cause: StdErrorType },
    /// Another retryable error
    #[error("Socket is not connected.")]
    NotConnected { payload: Option<Payload> },
}
impl<Payload: Debug + PartialEq> TransportErrors<Payload> {
    pub fn is_fatal(&self) -> bool {
        match self {
            TransportErrors::QuotaExhausted    {..} => true,
            TransportErrors::CannotReconnect   {..} => true,
            TransportErrors::ConnectionDropped {..} => false,
            TransportErrors::NotConnected { .. }    => false,
        }
    }
    /// Extracts the payload from the given error (panics if `self` is not a retryable error),
    pub fn split(self) -> (Payload, Self) {
        match self {
            TransportErrors::QuotaExhausted { payload, root_cause } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::QuotaExhausted {..}): parameter didn't have a payload"),
                  Self::QuotaExhausted { payload: None, root_cause } )
            },
            TransportErrors::ConnectionDropped { payload, root_cause } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::ConnectionDropped {..}): parameter didn't have a payload"),
                  Self::ConnectionDropped { payload: None, root_cause } )
            },
            TransportErrors::CannotReconnect { payload, root_cause } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::CannotReconnect {..}): parameter didn't have a payload"),
                  Self::CannotReconnect { payload: None, root_cause } )
            },
            TransportErrors::NotConnected { payload } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::NotConnected {..}): parameter didn't have a payload"),
                  Self::NotConnected { payload: None } )
            },
        }
    }
    pub fn for_payload<AnyRet>(&mut self, f: impl FnOnce(&mut Option<Payload>) -> AnyRet) -> AnyRet {
        match self {
            TransportErrors::QuotaExhausted    { ref mut payload, .. } => f(payload),
            TransportErrors::ConnectionDropped { ref mut payload, .. } => f(payload),
            TransportErrors::CannotReconnect   { ref mut payload, .. } => f(payload),
            TransportErrors::NotConnected      { ref mut payload, .. } => f(payload),
        }
    }
}
impl<Payload: Debug + PartialEq> PartialEq for TransportErrors<Payload> {
    fn eq(&self, other: &Self) -> bool {
        match self {
            TransportErrors::QuotaExhausted    { payload, .. } => if let TransportErrors::QuotaExhausted    { payload: other_payload, ..} = other { payload == other_payload } else { false },
            TransportErrors::ConnectionDropped { payload, .. } => if let TransportErrors::ConnectionDropped { payload: other_payload, ..} = other { payload == other_payload } else { false },
            TransportErrors::CannotReconnect   { payload, .. } => if let TransportErrors::CannotReconnect   { payload: other_payload, ..} = other { payload == other_payload } else { false },
            TransportErrors::NotConnected      { payload, .. } => if let TransportErrors::NotConnected      { payload: other_payload }    = other { payload == other_payload } else { false },
        }
    }
}
