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
use keen_retry::{RetryConsumerResult, RetryProcedureResult, RetryResult};
use std::{
    fmt::Debug,
    time::{Duration, SystemTime},
    sync::Arc,
};
use std::future::Future;
use std::pin::Pin;
use log::{error, info, warn};


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

/// Demonstrates how to use the pattern presented in the free `keen-retry` book
/// and implemented by [keen_broadcast()] --
/// useful for operations containing sub-operations that may be completed in steps
#[tokio::test]
async fn partial_completion_with_continuation_closure() {
    let expected_fatal_error = TransportErrors::QuotaExhausted { payload: None, root_cause: Box::from("You abused sending. Please don't try to send anything else today or you will be banned!") };
    if let Err(mut fatal_errors_list) = keen_broadcast("Tell everyone!").await {
        assert_eq!(fatal_errors_list.len(), 1, "There should be a single target for which we couldn't send due to a fatal error");
        let observed_fatal_error = fatal_errors_list.pop();
        assert_eq!(observed_fatal_error, Some(expected_fatal_error), "The failed target reported the wrong error")
    } else {
        panic!("`keen_broadcast()` reported it was able send the message to all targets, which isn't right")
    }
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
async fn keen_connect_to_server(socket: &Arc<Socket>) -> Result<(), ConnectionErrors> {
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
async fn keen_send<T: Debug + PartialEq>

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
async fn keen_receive(socket: &Arc<Socket>) -> Result<&'static str, TransportErrors<&'static str>> {
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

/// Simulates an application specific retryable API demonstrating the
/// "Partial Completion with Continuation Closure" pattern -- described
/// in details in the free `keen-retry` book.\
/// Here, the caller doesn't know the list of [Socket]s to broadcast to -- it is a knowledge
/// restricted to this method. For such, a closure is used in the place of the "payload",
/// allowing a different usage semantic: the "payload" won't be re-applied to the operation,
/// but, instead, it contains a callable object that knows what payloads to reapply to which
/// operations.\
/// This pattern is very useful in advanced retrying logics.
fn broadcast_continuation_closure<T: Clone + Debug + PartialEq + 'static>
                       (payload: T)
                       -> impl FnMut(()) -> Pin < Box < dyn Future < Output=RetryProcedureResult<Box<Vec<TransportErrors<T>>>> > > >  {

    let mut list_of_targets_ptr = Box::into_raw(Box::new(vec![
        // this one will fail transiently on the first connection
        Socket::new(2, 999, 1, 999),
        // this one will fail transiently on the first sending attempt
        Socket::new(1, 999, 2, 999),
        // this one will fail fatably, always, and won't ever be sent -- not even after all retries
        Socket::new(1, 999, 999, 1),
    ]));
    let mut fatal_failures_ptr = Box::into_raw(Box::new(Vec::new()));

    info!("Starting BROADCAST");

    move |_| {
        let payload = payload.clone();
        // if you don't like pointers & the unsafe here, use Arc<Mutex<>> instead.
        // This ownership problem is not present for closures that are not async
        let list_of_targets = unsafe { &mut *list_of_targets_ptr };
        let fatal_failures = unsafe { &mut *fatal_failures_ptr };

        info!("Continueing with the BROADCAST of {} items -- {} fatably failed so far", list_of_targets.len(), fatal_failures.len());

        Box::pin(async move {
            let mut transient_failed_targets = Vec::new();
            let mut transient_failures = Vec::new();
            for target in list_of_targets.drain(..) {
                // notice the send operation here has a different retrying logic -- we don't retry on individual sends
                // (this is both to show case different retrying policies in different contexts and to optimize the operation,
                //  as retrying is postponed as much as possible to the future, giving the target peer as much time to recover
                //  as possible without delaying our broadcast to others)
                let send_result = target.send(payload.clone()).await;
                match send_result {
                    RetryResult::Transient { input: _, error } => {
                        transient_failed_targets.push(target);
                        transient_failures.push(error)
                    },
                    RetryResult::Fatal { input, error } => {
                        fatal_failures.push(error);
                    },
                    _ => (),
                }
            }
            if transient_failed_targets.len() == 0 && fatal_failures.len() == 0 {
                warn!("Ending BROADCAST with 100% success");
                RetryResult::Ok { reported_input: (), output: () }
            } else if transient_failed_targets.len() > 0 {
                for target in transient_failed_targets.drain(..) {
                    list_of_targets.push(target);
                }
                RetryResult::Transient { input: (), error: Box::from(transient_failures) }
            } else {
                warn!("Ending BROADCAST with {} fatal failure(s)", fatal_failures.len());
                RetryResult::Fatal { input: (), error: unsafe { Box::from_raw(fatal_failures_ptr) } }
            }
        })
    }
}

/// Demonstrates the "Partial Completion with Continuation Closure" pattern:\
/// Here, the caller doesn't know the list of [Socket]s to broadcast to -- it is a knowledge
/// restricted to this method. For such, a closure is used in the place of the "payload",
/// allowing a different usage semantic: the "payload" won't be re-applied to the operation,
/// but, instead, it contains a callable object that knows what payloads to reapply to which
/// operations.\
/// This pattern is very useful in advanced retrying logics.
async fn keen_broadcast<T: Debug + PartialEq + Clone + 'static>
                       (payload: T)
                       -> Result<(), Box<Vec<TransportErrors<T>>>>  {

    let mut continuation_closure = broadcast_continuation_closure(payload);
    continuation_closure(()).await
        .retry_with_async(continuation_closure)
        .with_exponential_jitter(keen_retry::ExponentialJitter::FromBackoffRange { backoff_range_millis: 1000..=10000, re_attempts: 10, jitter_ratio: 0.2 })
        .await
        .inspect_unrecoverable(|_, remaining_errors, fatal_failures| warn!("Fatal failures found during the Broadcast: {}", keen_retry::loggable_retry_errors(fatal_failures)))
        .into_result()
}