//! Take it from tests from reactive-messaging/mutiny

use keen_retry::RetryConsumerResult;
use std::{
    fmt::Debug,
    time::Duration,
    sync::atomic::{
        AtomicI32,
        Ordering::Relaxed,
    }
};
use thiserror::Error;


type ErrorType = Box<dyn std::error::Error + Send + Sync>;


/// Shows how to make a fully-fledged instrumentation, if retrying is necessary,
/// wthout incurring into any extra code if no retrying takes place (in which case, no instrumentation is done)
#[test]
fn zero_cost_abstractions() -> Result<(), ErrorType> {


    #[derive(Error, Debug)]
    enum SocketErrorType {
        #[error("Wrong Credentials.")]
        WrongCredentials,
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

    struct Socket {
        is_connected: bool,
    }
    impl Socket {
        pub fn connect_to_server(&mut self) -> Result<(), SocketErrorType> {
            self.connect_to_server_retry()
                .retry_with(|_| Ok(self.connect_to_server_retry()))
                .with_delays([10,30,50,100,1,1,1,1,1,1,1,1,1].into_iter().map(|millis| Duration::from_millis(millis)))
                .inspect_recovered(|payload, retry_errors_list| println!("connect_to_server(): successfully connected after retrying {} times (failed retries: {:?})", retry_errors_list.len(), retry_errors_list))
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
        pub fn new() -> Self {
            Self { is_connected: false }
        }
        pub fn connect_to_server_raw(&mut self) -> Result<(), SocketErrorType> {
            static SUCCESS_LATCH_COUNTER:       AtomicI32 = AtomicI32::new(10);
            static FATAL_FAILURE_LATCH_COUNTER: AtomicI32 = AtomicI32::new(10);
            if SUCCESS_LATCH_COUNTER.fetch_sub(1, Relaxed) == 0 {
                self.is_connected = true;
                Ok(())
            } else if FATAL_FAILURE_LATCH_COUNTER.fetch_sub(1, Relaxed) == 0 {
                Err(SocketErrorType::WrongCredentials)
            } else {
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
            .inspect_given_up(|payload, retry_errors_list| println!("`send()` FAILED after exhausting all {} retrying attempts (failed retries: {:?})", retry_errors_list.len(), retry_errors_list))
            .inspect_unrecoverable(|_payload, retry_errors_list, fatal_error| println!("`send()`: fatal error after retrying {} time(s): {:?}", retry_errors_list.len()+1, fatal_error))
            .inspect_recovered(|loggable_payload, retry_errors_list| println!("send(): successfully sent {}", loggable_payload))
            .into()
        }

    let mut socket = Socket::new();
    send_retry(&mut socket, "holly molly")?;

    Ok(())

}