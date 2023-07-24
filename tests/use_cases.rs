//! Demonstrates how the `keen-retry` library can be integrated with the Rust's original `Result<O,E>` API.
//! 
//! Shows all possible use cases in which retries could be useful (both sync & async):
//!   - using all retry instrumentation features
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
use thiserror::Error;


type ErrorType = Box<dyn std::error::Error + Send + Sync>;

/// Uses our fake [Socket], implementing a simple retrying mechanism (no instrumentations),
/// to test all possible scenarios: 
#[test]
fn simple_retries() -> Result<(), ErrorType> {

    let case_name = "1) Good at the first shot:";
    // println!("\n{}", case_name);
    let mut socket = Socket::new(13, 1, 2);
    let result = socket.connect_to_server();
    assert_eq!(result, Ok(()), "In '{}'", case_name);
    assert_eq!(socket.is_connected, true, "In '{}'", case_name);

    let case_name = "2) Failed fatably at the first shot:";
    // println!("\n{}", case_name);
    let mut socket = Socket::new(13, 2, 1);
    let result = socket.connect_to_server();
    assert_eq!(result, Err(SocketErrorType::WrongCredentials), "In '{}'", case_name);
    assert_eq!(socket.is_connected, false, "In '{}'", case_name);

    let case_name = "3) Recovered from failure on the 10th attempt:";
    // println!("\n{}", case_name);
    let mut socket = Socket::new(13, 10, 11);
    let result = socket.connect_to_server();
    assert_eq!(result, Ok(()), "In '{}'", case_name);
    assert_eq!(socket.is_connected, true, "In '{}'", case_name);

    let case_name = "4) Failed due to gaving up retryng after exceeding 13 attempts:";
    // println!("\n{}", case_name);
    let mut socket = Socket::new(13, 15, 16);
    let result = socket.connect_to_server();
    assert_eq!(result, Err(SocketErrorType::ServerTooBusy), "In '{}'", case_name);
    assert_eq!(socket.is_connected, false, "In '{}'", case_name);

    let case_name = "5) Failed fatably during a retry attempt:";
    // println!("\n{}", case_name);
    let mut socket = Socket::new(13, 11, 10);
    let result = socket.connect_to_server();
    assert_eq!(result, Err(SocketErrorType::WrongCredentials), "In '{}'", case_name);
    assert_eq!(socket.is_connected, false, "In '{}'", case_name);

    Ok(())
}

/// Shows how to make a fully-fledged instrumentation, if retrying is necessary,
/// wthout incurring into any extra code if no retrying takes place (in which case, no instrumentation is done)
#[test]
fn zero_cost_abstractions() -> Result<(), ErrorType> {
    let case_name = "1) Good at the first shot:";
    println!("\n{}", case_name);
    let mut socket = Socket::new(13, 1, 2);
    let result = send_retry(&mut socket, case_name);
    assert_eq!(result, Ok(()), "In '{}'", case_name);
    assert_eq!(socket.is_connected, true, "In '{}'", case_name);

    Ok(())
}

#[derive(Error, Debug, PartialEq)]
enum SocketErrorType {
    /// A fatal error
    #[error("Wrong Credentials.")]
    WrongCredentials,
    /// A retryable error (becomes fatal if the number of retry attempts gets exhausted)
    #[error("Server too busy. Try again later.")]
    ServerTooBusy,
}
impl SocketErrorType {
    pub fn is_fatal(&self) -> bool {
        match self {
            SocketErrorType::ServerTooBusy    => false,
            SocketErrorType::WrongCredentials => true,
        }
    }
}


const SOCKET_RETRY_DELAY_MILLIS: [u64; 13] = [10,30,50,100,1,1,1,1,1,1,1,1,1];

/// Example of how to implement a retry mechanism free of instrumentation:
/// In this hipothetical scenario, when connecting, many retryable errors happen until
/// either a `success_latch_counter` or `fatal_failure_latch_counter` counts down to zero.
struct Socket {
    is_connected:                  bool,
    max_retries:                   u32,
    success_latch_countdown:       i32,
    fatal_failure_latch_countdown: i32,
    attempts:                      u32,
}

impl Socket {

    pub fn connect_to_server(&mut self) -> Result<(), SocketErrorType> {
        self.connect_to_server_retry()
            .retry_with(|_| Ok(self.connect_to_server_retry()))
            .with_delays(SOCKET_RETRY_DELAY_MILLIS.into_iter().map(|millis| Duration::from_millis(millis)))
            .inspect_recovered(|_payload, retry_errors_list| println!("connect_to_server(): successfully connected after retrying {} times (failed attempts: {:?})", retry_errors_list.len(), retry_errors_list))
            .into()
    }

    pub fn connect_to_server_retry(&mut self) -> RetryConsumerResult<(), (), SocketErrorType> {
        self.connect_to_server_raw()
            .map_or_else(|error| match error.is_fatal() {
                            true =>  RetryConsumerResult::Fatal { payload: (), error },
                            false => RetryConsumerResult::Retry { payload: (), error },
                         },
                         |_| RetryConsumerResult::Ok {payload: ()})
    }

    pub fn new(max_retries: u32, success_latch_countdown: i32, fatal_failure_latch_countdown: i32) -> Self {
        Self {
            is_connected: false,
            max_retries,
            success_latch_countdown,
            fatal_failure_latch_countdown,
            attempts: 0,
         }
    }

    pub fn connect_to_server_raw(&mut self) -> Result<(), SocketErrorType> {
        self.success_latch_countdown -= 1;
        self.fatal_failure_latch_countdown -= 1;
        self.attempts += 1;
        if self.success_latch_countdown == 0 {
            self.is_connected = true;
            Ok(())
        } else if self.fatal_failure_latch_countdown == 0 {
            self.is_connected = false;
            Err(SocketErrorType::WrongCredentials)
        } else {
            self.is_connected = false;
            Err(SocketErrorType::ServerTooBusy)
        }
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected
    }

}

fn send<T:Debug>(socket: &Socket, payload: T) -> RetryConsumerResult</*loggable payload*/String, T, ErrorType> {
    let loggable_payload = format!("{:?}", payload);
    if true /*channel.is_full()*/ {
        RetryConsumerResult::Retry { payload, error: Box::from(format!("Channel is still full")) }
    } else {
        Ok(())//channel.send(T)
            .map_or_else(|error| RetryConsumerResult::Fatal { payload, error },
                         |_| RetryConsumerResult::Ok {payload: loggable_payload})
    }
}

fn send_retry(mut socket: &mut Socket, payload: &str) -> Result<String, ErrorType> {
    send(socket, payload)
        .inspect_fatal(|payload, fatal_err| println!("`send()`: fatal error (won't retry): {:?}", fatal_err))
//            .map_retry(|payload, err| ( (payload, format!("{:?}", payload)), err))
//            .inspect_retry(|(payload, loggable_payload), err| {})
        .retry_with(|payload| {
            // verify the connection
            if !socket.is_connected() {
                socket.connect_to_server()
                    .map_err(|err| format!("Send failed (gave up retrying after a failed reconnection attempt): {}", err))?;   // `connect_to_server()` itself implement a similar retrying mechanism. map_err is used to better error messages
            }
            let loggable_payload = format!("{:?}", payload);
            Ok(send(socket, payload)
                .map_ok_result(|_| loggable_payload))
        })
        // the above statement returns a KeenRetryExecutor<RetryResult<_>>, which is executed by one of its methods
        .with_delays([10,30,50,100].into_iter().map(|millis| Duration::from_millis(millis)))
        // from this point down, we have a KeenRetryResult object -- a resolved RetryResult in which retrying is no longer possible... just for fetching the values
        .inspect_given_up(|payload, retry_errors_list| println!("`send()` FAILED after exhausting all {} retrying attempts (failed attempts: {:?})", retry_errors_list.len(), retry_errors_list))
        .inspect_unrecoverable(|_payload, retry_errors_list, fatal_error| println!("`send()`: fatal error after retrying {} time(s): {:?}", retry_errors_list.len()+1, fatal_error))
        .inspect_recovered(|loggable_payload, retry_errors_list| println!("send(): successfully sent {}", loggable_payload))
        .into()
}
