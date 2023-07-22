//! Take it from tests from reactive-messaging/mutiny

use std::fmt::Debug;
use std::time::Duration;
use keen_retry::RetryConsumerResult;

#[test]
fn test() {

    type ErrorType = Box<dyn std::error::Error + Send + Sync + 'static>;

    struct Socket {

    }
    impl Socket {
        pub fn connect_to_server_retry() -> Result<Self, ErrorType> {
            Err(Box::from(format!("Hardcoded error")))
        }
        pub fn is_connected(&self) -> bool {false}
    }

    fn send<T:Debug>(socket: &Socket, payload: T) -> RetryConsumerResult</*loggable payload*/String, T, ErrorType> {
        let loggable_payload = format!("{:?}", payload);
        if true /*channel.is_full()*/ {
            RetryConsumerResult::Retry { payload, error: Box::from(format!("Channel is full")) }
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
                    *socket = Socket::connect_to_server_retry()
//                        .map_err(|err| Box::from(format!("Send failed (gave up retrying) due to not being able to reconnect to the server due to a fatal error: {}", err)))?;   // `connect_to_server()` itself implement a similar retrying mechanism. map_err is used to better error messages
                    ?;
                }
                let loggable_payload = format!("{:?}", payload);
                Ok(send(socket, payload)
                    .map_ok_result(|_| loggable_payload))
            })
            // the above statement returns a KeenRetryExecutor<RetryResult<_>>, which is executed by one of its methods
            .with_delays([10,30,50,100].into_iter().map(|millis| Duration::from_millis(millis)))
            // from this point down, we have a KeenRetryResult object -- a resolved RetryResult in which retrying is no longer possible... just for fetching the values
            .inspect_given_up(|payload, retry_errors_list| println!("`send()` has exhausted all {} retrying attempts", retry_errors_list.len()))
            .inspect_unrecoverable(|_payload, retry_errors_list, fatal_error| println!("`send()`: fatal error after retrying {} time(s): {:?}", retry_errors_list.len()+1, fatal_error))
            .inspect_recovered(|loggable_payload, retry_errors_list| println!("send(): successfully sent {}", loggable_payload))
            .into()
        }

    let mut socket = Socket {};
    let r = send_retry(&mut socket, "holly molly");

}