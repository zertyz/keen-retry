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

use keen_retry::RetryConsumerResult;
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
    let expected = Err(SendingErrors::QuotaExhausted { payload: Some(to_send()), root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 1, 11, 13, 10, 1);
    socket.connect_to_server().await?;
    let result = socket.send(to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "3.1) Recovered from failure on the 10th attempt (connection was initially OK)";
    println!("\n{}:", case_name);
    let expected = Ok(format!("{:?}", MyPayload { message: case_name }));
    let mut socket = Socket::new(13, 1, 999, 13, 10, 11);
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
    let expected = Err(SendingErrors::ConnectionDropped { payload: None, root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 1, 999, 13, 15, 16);
    socket.connect_to_server().await?;
    let result = socket.send(MyPayload { message: case_name }).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "5.1) Failed fatably during the 10th retry attempt (in `send()`)";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(SendingErrors::QuotaExhausted { payload: Some(to_send()), root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 1, 999, 13, 999, 10);
    socket.connect_to_server().await?;
    let result = socket.send(to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    let case_name = "5.2) Failed fatably during a retry attempt (on reconnection)";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(SendingErrors::CannotReconnect { payload: Some(to_send()), root_cause: format!("any root cause....").into() });
    let mut socket = Socket::new(13, 999, 999, 13, 15, 16);
    let result = socket.send(to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert_eq!(socket.is_connected(), false, "In '{}'", case_name);

    Ok(())
}

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
            ConnectionErrors::ServerTooBusy    => false,
            ConnectionErrors::WrongCredentials => true,
        }
    }
}

#[derive(Error, Debug)]
enum SendingErrors<Payload: Debug + PartialEq> {
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
impl<Payload: Debug + PartialEq> SendingErrors<Payload> {
    pub fn is_fatal(&self) -> bool {
        match self {
            SendingErrors::QuotaExhausted    {..} => true,
            SendingErrors::ConnectionDropped {..} => false,
            SendingErrors::CannotReconnect   {..} => true,
            SendingErrors::NotConnected { .. }    => false,
        }
    }
    /// Extracts the payload from the given error (panics if `self` is not a retryable error),
    pub fn split(self) -> (Payload, Self) {
        match self {
            SendingErrors::QuotaExhausted { payload, root_cause } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::QuotaExhausted {..}): parameter didn't have a payload"),
                  Self::QuotaExhausted { payload: None, root_cause } )
            },
            SendingErrors::ConnectionDropped { payload, root_cause } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::ConnectionDropped {..}): parameter didn't have a payload"),
                  Self::ConnectionDropped { payload: None, root_cause } )
            },
            SendingErrors::CannotReconnect { payload, root_cause } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::CannotReconnect {..}): parameter didn't have a payload"),
                  Self::CannotReconnect { payload: None, root_cause } )
            },
            SendingErrors::NotConnected { payload } => {
                ( payload.expect("SendingErrors: extract_retry_payload(SendingErrors::NotConnected {..}): parameter didn't have a payload"),
                  Self::NotConnected { payload: None } )
            },
        }
    }
    pub fn for_payload<AnyRet>(&mut self, f: impl FnOnce(&mut Option<Payload>) -> AnyRet) -> AnyRet {
        match self {
            SendingErrors::QuotaExhausted    { ref mut payload, .. } => f(payload),
            SendingErrors::ConnectionDropped { ref mut payload, .. } => f(payload),
            SendingErrors::CannotReconnect   { ref mut payload, .. } => f(payload),
            SendingErrors::NotConnected      { ref mut payload, .. } => f(payload),
        }
    }
}
impl<Payload: Debug + PartialEq> PartialEq for SendingErrors<Payload> {
    fn eq(&self, other: &Self) -> bool {
        match self {
            SendingErrors::QuotaExhausted    { payload, .. } => if let SendingErrors::QuotaExhausted    { payload: other_payload, ..} = other { payload == other_payload } else { false },
            SendingErrors::ConnectionDropped { payload, .. } => if let SendingErrors::ConnectionDropped { payload: other_payload, ..} = other { payload == other_payload } else { false },
            SendingErrors::CannotReconnect   { payload, .. } => if let SendingErrors::CannotReconnect   { payload: other_payload, ..} = other { payload == other_payload } else { false },
            SendingErrors::NotConnected      { payload, .. } => if let SendingErrors::NotConnected      { payload: other_payload }    = other { payload == other_payload } else { false },
        }
    }
}



const RETRY_DELAY_MILLIS: [u64; 13] = [10,30,50,100,1,1,1,1,1,1,1,1,1];

/// Example of how to implement a retry mechanism free of instrumentation:
/// In this hipothetical scenario, when connecting, many retryable errors happen until
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
            .with_delays(RETRY_DELAY_MILLIS.into_iter().map(|millis| Duration::from_millis(millis)))
            .await
            .inspect_recovered(|_payload, retry_errors_list| println!("connect_to_server(): successfully connected after retrying {} times (failed attempts: {:?})", retry_errors_list.len(), retry_errors_list))
            .into()
    }

    /// Shows off a rather "complex" retrying & instrumentation logic, but still zero-cost when not retrying:
    async fn send<T: Debug + PartialEq>

                 (self: &Arc<Self>,
                  payload: T)

                 -> Result<String, SendingErrors<T>> {

        let loggable_payload = format!("{:?}", payload);
        self.send_retry(payload)
            .map_ok_result(|_| (loggable_payload, Duration::ZERO))
            .inspect_fatal(|payload, fatal_err| println!("`send()`: fatal error (won't retry): {:?}", fatal_err))
            .map_retry_payload(|payload| ( (payload, SystemTime::now() )) )
            .retry_with_async(|(payload, retry_start)| async move {
                let loggable_payload = format!("{:?}", &payload);
                if !self.is_connected() {
                    if let Err(err) = self.connect_to_server().await {
                        println!("`send({loggable_payload})`: Error attempting to reconnect: {}", err);
                        return RetryConsumerResult::Fatal { payload: (payload, retry_start), error: SendingErrors::CannotReconnect {payload: None, root_cause: err.into()} };
                    }
                }
                self.send_retry(payload)
                    .map_ok_result(|_| (loggable_payload, retry_start.elapsed().unwrap()) )
                    .map_retry_payload(|payload| (payload, retry_start) )
            })
            .with_delays(RETRY_DELAY_MILLIS.into_iter().map(|millis| Duration::from_millis(millis)))
            .await
            .inspect_given_up(|(payload, retry_start), retry_errors_list| println!("`send({:?})` FAILED after exhausting all {} retrying attempts in {:?} ({:?})",  payload, retry_errors_list.len(), retry_start.elapsed(), retry_errors_list))
            .inspect_unrecoverable(|payload_and_retry_start, retry_errors_list, fatal_error| {
                let (payload, retry_start) = payload_and_retry_start.as_ref().map_or_else(|| (None, None), |v| (Some(&v.0), Some(v.1.elapsed().unwrap())));
                println!("`send({:?})`: fatal error after trying {} time(s) in {:?}: {:?}", payload, retry_errors_list.len()+1, retry_start, fatal_error)
            })
            .inspect_recovered(|(loggable_payload, duration), retry_errors_list| println!("send({loggable_payload}): succeeded after trying {} time(s) in {:?}: {:?}", retry_errors_list.len()+1, duration, retry_errors_list))
            // remaps the types back to their originals, so this will be conversible into a `Result<>`
            .map_retry_payload(|(loggable_payload, retry_start)| loggable_payload)
            .map_ok_result(|(loggable_payload, duration)| loggable_payload)
            // go an extra mile to demonstrate how to place back the payloads of failed requests in `Option<>` field
            .map_errors(|mut fatal_error, payload| {
                            fatal_error.for_payload(|mut_payload| payload.and_then(|payload| mut_payload.replace(payload)));
                            fatal_error
                        },
                        |unmapped| unmapped)
            .into()
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Relaxed)
    }


    // low level functions //
    /////////////////////////

    async fn connect_to_server_retry(self: &Arc<Self>) -> RetryConsumerResult<(), (), ConnectionErrors> {
        self.connect_to_server_raw().await
            .map_or_else(|error| match error.is_fatal() {
                            true  => RetryConsumerResult::Fatal { payload: (), error },
                            false => RetryConsumerResult::Retry { payload: (), error },
                         },
                         |_| RetryConsumerResult::Ok {payload: ()})
    }

    /// Simulates a real connection, failing according to the configuration
    async fn connect_to_server_raw(self: &Arc<Self>) -> Result<(), ConnectionErrors> {
        self.connection_attempts.fetch_add(1, Relaxed);
        if self.connection_success_latch_countdown.fetch_sub(1, Relaxed) == 0 {
            self.connection_fatal_failure_latch_countdown.fetch_sub(1, Relaxed);
            self.is_connected.store(true, Relaxed);
            Ok(())
        } else if self.connection_fatal_failure_latch_countdown.fetch_sub(1, Relaxed) == 0 {
            self.is_connected.store(false, Relaxed);
            Err(ConnectionErrors::WrongCredentials)
        } else if self.connection_success_latch_countdown.load(Relaxed) < 0 {
            self.is_connected.store(true, Relaxed);
            Ok(())
        } else {
            self.is_connected.store(false, Relaxed);
            Err(ConnectionErrors::ServerTooBusy)
        }
    }

    fn send_retry<T: Debug + PartialEq>

                 (self: &Arc<Self>,
                  payload: T)

                 -> RetryConsumerResult<(), T, SendingErrors<T>> {

        self.send_raw(payload)
            .map_or_else(|error| match error.is_fatal() {
                             true  => {
                                 let (payload, error) = error.split();
                                 RetryConsumerResult::Fatal { payload, error }
                             },
                             false => {
                                 let (payload, error) = error.split();
                                 RetryConsumerResult::Retry { payload, error }
                             },
                         },
                         |_| RetryConsumerResult::Ok { payload: () })
    }

    fn send_raw<T: Debug + PartialEq>

               (self: &Arc<Self>,
                payload: T)

               -> Result<(), SendingErrors<T>> {

        if !self.is_connected.load(Relaxed) {
            Err(SendingErrors::NotConnected { payload: Some(payload) })
        } else {
            self.sending_attempts.fetch_add(1, Relaxed);
            if self.sending_success_latch_countdown.fetch_sub(1, Relaxed) == 0 {
                self.sending_fatal_failure_latch_countdown.fetch_sub(1, Relaxed);
                Ok(())
            } else if self.sending_fatal_failure_latch_countdown.fetch_sub(1, Relaxed) == 0 {
                self.is_connected.store(false, Relaxed);
                Err(SendingErrors::QuotaExhausted { payload: Some(payload), root_cause: format!("You abused sending. Please don't try to send anything else today or you will be banned!").into() })
            } else {
                self.is_connected.store(false, Relaxed);
                Err(SendingErrors::ConnectionDropped { payload: Some(payload), root_cause: format!("Socket read error (-1)").into() })
            }
        }
    }

}

#[derive(Debug, PartialEq)]
struct MyPayload {
    message: &'static str,
}