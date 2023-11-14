//! Demonstrates how the `keen-retry` library can be integrated back and forth with the Rust's original `Result<O,E>` API.
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

mod external_lib;
use external_lib::*;
use keen_retry::RetryResult;
use std::{
    fmt::Debug,
    time::{Duration, SystemTime},
    sync::Arc,
};
use log::{error, warn};


type StdErrorType = Box<dyn std::error::Error + Send + Sync>;


#[ctor::ctor]
fn suite_setup() {
    simple_logger::SimpleLogger::new().with_utc_timestamps().init().unwrap_or_else(|_| eprintln!("--> LOGGER WAS ALREADY STARTED"));
}

/// Uses our fake [Socket] connection, implementing a simple retrying mechanism (no instrumentations),
/// to test all possible scenarios: 
#[tokio::test]
async fn simple_retries() -> Result<(), StdErrorType> {

    let case_name = "1) Good at the first shot";
    // println!("\n{}:", case_name);
    let socket = Socket::new(1, 2, 0, 0);
    let result = keen_connect_to_server(&socket).await;
    assert_eq!(result, Ok(()), "In '{}'", case_name);
    assert!(socket.is_connected(), "In '{}'", case_name);

    let case_name = "2) Failed fatably at the first shot";
    // println!("\n{}:", case_name);
    let socket = Socket::new(2, 1, 0, 0);
    let result = keen_connect_to_server(&socket).await;
    assert_eq!(result, Err(ConnectionErrors::WrongCredentials), "In '{}'", case_name);
    assert!(!socket.is_connected(), "In '{}'", case_name);

    let case_name = "3) Recovered from failure on the 10th attempt";
    // println!("\n{}:", case_name);
    let socket = Socket::new(10, 11, 0, 0);
    let result = keen_connect_to_server(&socket).await;
    assert_eq!(result, Ok(()), "In '{}'", case_name);
    assert!(socket.is_connected(), "In '{}'", case_name);

    let case_name = "4) Failed due to give up retrying after exceeding 13 attempts";
    // println!("\n{}:", case_name);
    let socket = Socket::new(15, 16, 0, 0);
    let result = keen_connect_to_server(&socket).await;
    assert_eq!(result, Err(ConnectionErrors::ServerTooBusy), "In '{}'", case_name);
    assert!(!socket.is_connected(), "In '{}'", case_name);

    let case_name = "5) Failed fatably during the 10th retry attempt";
    // println!("\n{}:", case_name);
    let socket = Socket::new(11, 10, 0, 0);
    let result = keen_connect_to_server(&socket).await;
    assert_eq!(result, Err(ConnectionErrors::WrongCredentials), "In '{}'", case_name);
    assert!(!socket.is_connected(), "In '{}'", case_name);

    Ok(())
}

/// Uses our fake [Socket] sending to show how to make a fully-fledged instrumentation, if retrying is necessary,
/// without incurring into any extra code if no retrying takes place (in which case, no instrumentation is done)
#[tokio::test]
async fn zero_cost_abstractions() -> Result<(), StdErrorType> {

    let case_name = "1) Good at the first shot";
    println!("\n{}:", case_name);
    let expected = Ok(format!("{:?}", MyPayload { message: case_name }));
    let socket = Socket::new(1, 11, 1, 2);
    keen_connect_to_server(&socket).await?;
    let result = keen_send(&socket, MyPayload { message: case_name }).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert!(socket.is_connected(), "In '{}'", case_name);

    let case_name = "2) Failed fatably at the first shot";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::QuotaExhausted { payload: Some(to_send()), root_cause: String::from("any root cause....").into() });
    let socket = Socket::new(0, 11, 10, 1);
    keen_connect_to_server(&socket, ).await?;
    let result = keen_send(&socket, to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert!(!socket.is_connected(), "In '{}'", case_name);

    let case_name = "3.1) Recovered from failure on the 10th attempt (connection was initially OK)";
    println!("\n{}:", case_name);
    let expected = Ok(format!("{:?}", MyPayload { message: case_name }));
    let socket = Socket::new(0, 999, 10, 11);
    keen_connect_to_server(&socket, ).await?;
    let result = keen_send(&socket, MyPayload { message: case_name }).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert!(socket.is_connected(), "In '{}'", case_name);

    let case_name = "3.2) Recovered from failure on the 10th attempt (connection was initially NOT OK and will only succeed at the 5th attempt)";
    println!("\n{}:", case_name);
    let expected = Ok(format!("{:?}", MyPayload { message: case_name }));
    let socket = Socket::new(5, 999, 5, 11);
    let result = keen_send(&socket, MyPayload { message: case_name }).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert!(socket.is_connected(), "In '{}'", case_name);

    let case_name = "4) Failed due to give up retrying `send()` after exceeding 13 attempts";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::ConnectionDropped { payload: Some(to_send()), root_cause: String::from("any root cause....").into() });
    let socket = Socket::new(1, 999, 15, 16);
    keen_connect_to_server(&socket, ).await?;
    let result = keen_send(&socket, to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert!(socket.is_connected(), "In '{}'", case_name);

    let case_name = "5.1) Failed fatably during the 10th retry attempt (in `send()`)";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::QuotaExhausted { payload: Some(to_send()), root_cause: String::from("any root cause....").into() });
    let socket = Socket::new(1, 999, 999, 10);
    keen_connect_to_server(&socket).await?;
    let result = keen_send(&socket, to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert!(!socket.is_connected(), "In '{}'", case_name);

    let case_name = "5.2) Failed fatably during a retry attempt (on reconnection)";
    println!("\n{}:", case_name);
    let to_send = || MyPayload { message: case_name };
    let expected = Err(TransportErrors::CannotReconnect { payload: Some(to_send()), root_cause: String::from("any root cause....").into() });
    let socket = Socket::new(999, 10, 15, 16);
    let result = keen_send(&socket, to_send()).await;
    assert_eq!(result, expected, "In '{}'", case_name);
    assert!(!socket.is_connected(), "In '{}'", case_name);

    Ok(())
}

/// Demonstrates that we may not care if a function uses the `keen-retry` API or the standard Rust `Result<>`.\
/// Notice that the `*_retry` functions, from [Socket] are used instead of our local wrappers `keen_*()`
#[tokio::test]
async fn optable_api() -> Result<(), StdErrorType> {
    let case_name = "1) Ok operation";
    println!("\n{}:", case_name);
    let socket = Socket::new(1, 11, 1, 2);
    socket.connect_to_server().await.into_result()?;
    assert!(socket.receive().is_ok(), "Operation errored when it shouldn't at {case_name}");

    let case_name = "2) Err operation";
    println!("\n{}:", case_name);
    let socket = Socket::new(1, 11, 1, 2);
    assert!(socket.receive().is_err(), "Operation didn't errored as it should at {case_name}");

    Ok(())
}

/// Demonstrates how to bail out of retrying a consumer operation, dropping our custom errors and remapping the payload to `Result::Err(payload)`
/// -- this is a common pattern used in zero-copy producers: to return an unconsumed input in case of failure.
#[tokio::test]
async fn bail_out_of_retrying() -> Result<(), StdErrorType> {
    let socket = Socket::new(1, 11, 1, 2);
    let result = socket.send_retryable("I want this back")
        .map_input_and_errors(|input, _retry_error| ((), input),
                              |input, _fatal_error| ((), input))
        .into_result();
    assert_eq!(result, Err("I want this back"), "mapping inputs and errors didn't work");
    Ok(())
}


/// Our test payload, used in `send()` operations
#[derive(Debug, PartialEq)]
struct MyPayload {
    message: &'static str,
}


// Pattern: implement a local wrapper to bring keen retry `ResolvedResult` back to standard `Result<>`,
//          defining the retry logic there.

/// Shows off a simple retry logic with a simple instrumentation, ensuring any retry
/// attempts wouldn't go on silently.\
/// This is the minimum recommended instrumentation, which would be lost after downgrading
/// the [keen_retry::ResolvedResult] to a standard `Result<>`.
pub async fn keen_connect_to_server(socket: &Arc<Socket>) -> Result<(), ConnectionErrors> {
    let cloned_socket = Arc::clone(socket);
    socket.connect_to_server().await
        .retry_with_async(|_| cloned_socket.connect_to_server())
        .with_exponential_jitter(keen_retry::ExponentialJitter::FromBackoffRange { backoff_range_millis: 10..=130, re_attempts: 10, jitter_ratio: 0.1 })
        .await
        .inspect_recovered(|_, _, retry_errors_list| warn!("`keen_connect_to_server()`: successfully connected after retrying {} times (failed attempts: [{}])",
                                                                               retry_errors_list.len(), keen_retry::loggable_retry_errors(retry_errors_list)))
        .into_result()
}

/// Shows off a rather "complex" retrying & instrumentation logic, but still zero-cost when not retrying.\
/// This method has a slightly different return type than the original:
///   * if successful, it returns a serialized version of the sent payload. It is for demonstration purposes
///     of the keen-retry API only. BTW, this kind of conversion is called `reported_input`, to be distinguished
///     from the original input -- only available on failures (as it was not consumed).
///   * if unsuccessful, the original and unconsumed input is placed in the `Err` returned.
/// As said, this method is bloated with `keen-retry` High Order Functions for demonstration purposes. For a more
/// realistic usage, see [keen_receive()].
pub async fn keen_send<T: Debug + PartialEq>

                      (socket: &Arc<Socket>,
                      payload: T)

                      -> Result<String, TransportErrors<T>> {

    let loggable_payload = format!("{:?}", payload);
    socket.send(payload).await
        .inspect_fatal(|payload, fatal_err|
            error!("`keen_send({payload:?})`: fatal error (won't retry): {fatal_err:?}"))
        .map_ok(|_, _| ((loggable_payload.clone(), Duration::ZERO), () ) )
        .map_input(|payload| ( loggable_payload, payload, SystemTime::now() ) )
        .retry_with_async(|(loggable_payload, payload, retry_start)| async move {
            socket.send(payload).await
                // notice the retry operation must map to the same types as the original operation:
                .map_ok(|_, _| ((loggable_payload.clone(), retry_start.elapsed().unwrap_or_default()), () ))
                .map_input(|payload| (loggable_payload, payload, retry_start) )
        })
        .with_exponential_jitter(keen_retry::ExponentialJitter::FromBackoffRange { backoff_range_millis: 0..=100, re_attempts: 10, jitter_ratio: 0.1 })
        .await
        .inspect_given_up(|(_loggable_payload, payload, retry_start), retry_errors_list, fatal_error|
            error!("`keen_send({payload:?})` FAILED after exhausting all {} retrying attempts in {:?} with error {fatal_error:?}. Previous transient failures were: [{}]",
                   retry_errors_list.len(), retry_start.elapsed().unwrap_or_default(), keen_retry::loggable_retry_errors(retry_errors_list)))
        .inspect_unrecoverable(|(_loggable_payload, payload, retry_start), retry_errors_list, fatal_error| {
            error!("`keen_send({payload:?})`: fatal error after trying {} time(s) in {:?}: {fatal_error:?} -- prior to that fatal failure, these retry attempts also failed: [{}]",
                   retry_errors_list.len()+1, retry_start.elapsed().unwrap_or_default(), keen_retry::loggable_retry_errors(retry_errors_list));
        })
        .inspect_recovered(|(loggable_payload, duration), _output, retry_errors_list|
            warn!("`keen_send({loggable_payload})`: succeeded after retrying {} time(s) in {:?}. Transient failures were: [{}]",
                  retry_errors_list.len(), duration, keen_retry::loggable_retry_errors(retry_errors_list)))
        // remaps the input types back to their originals, simulating we are interested in only part of it (that other part was useful only for instrumentation)
        .map_unrecoverable_input(|(_loggable_payload, payload, _retry_start)| payload)
        .map_reported_input(|(loggable_payload, _duration)| loggable_payload)
        // demonstrates how to map the reported input (`loggable_payload` in our case) into the output, so it will be available at final `Ok(loggable_payload)` result
        .map_reported_input_and_output(|reported_input, output| (output, reported_input) )
        // go an extra mile to demonstrate how to place back the payloads of failed requests in the `TransportErrors.*.payload` field, so the standard `Result::Err` can have it
        .map_input_and_errors(|payload, mut fatal_error| {
                                    fatal_error.for_payload(|mut_payload| mut_payload.replace(payload));
                                    ((), fatal_error)
                                },
                                |ignored| ignored)
        .into_result()
}


/// A more realistic demonstration of a possible real usage of the `keen-retry` API (in opposition to the fully fledged one in [keen_send()]).\
/// Here, apart from the retrying logic & constraints, you will find detailed instrumentation with measurements for the time spent in
/// backoffs + reattempts, if retrying kicks in.
pub async fn keen_receive(socket: &Arc<Socket>) -> Result<&'static str, TransportErrors<&'static str>> {
    socket.receive()
        .inspect_fatal(|_, fatal_err|
            println!("## `keen_receive()`: fatal error (won't retry): {:?}", fatal_err))
        .map_ok(|_, received| ( Duration::ZERO, received ))
        .map_input(|_| /*retry_start: */ SystemTime::now() )
        .retry_with_async(| retry_start| async move {
            if !socket.is_connected() {
                if let Err(err) = keen_connect_to_server(socket).await {
                    println!("## `keen_receive()`: Error attempting to reconnect (won't retry): {}", err);
                    return RetryResult::Fatal { input: retry_start, error: TransportErrors::CannotReconnect {payload: None, root_cause: err.into()} };
                }
            }
            socket.receive()
                .map_ok(|_, received| ( retry_start.elapsed().unwrap_or_default(), received ) )
                .map_input(|_| retry_start )
        })
        .with_exponential_jitter(keen_retry::ExponentialJitter::FromBackoffRange { backoff_range_millis: 0..=100, re_attempts: 10, jitter_ratio: 0.1 })
        .await
        .inspect_given_up(|retry_start, retry_errors_list, fatal_error|
            println!("## `keen_receive()` FAILED after exhausting all {} retrying attempts in {:?} with error {fatal_error:?}. Previous transient failures were: [{}]",
                     retry_errors_list.len(), retry_start.elapsed().unwrap_or_default(), keen_retry::loggable_retry_errors(retry_errors_list)))
        .inspect_unrecoverable(|retry_start, retry_errors_list, fatal_error| {
            println!("## `keen_receive()`: fatal error after trying {} time(s) in {:?}: {fatal_error:?} -- prior to that fatal failure, these retry attempts also failed: [{}]",
                     retry_errors_list.len()+1, retry_start.elapsed().unwrap_or_default(), keen_retry::loggable_retry_errors(retry_errors_list));
        })
        .inspect_recovered(|duration, _output, retry_errors_list|
            println!("## `keen_receive()`: succeeded after retrying {} time(s) in {:?}. Transient failures were: [{}]",
                     retry_errors_list.len(), duration, keen_retry::loggable_retry_errors(retry_errors_list)))
        .into_result()

}