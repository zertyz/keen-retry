//! This module simulates an external library that uses the `keen-retry` Library API, where our
//! fictitious library is able to succeed, and transiently / fatally fail, akin to [crate::RetryResult],
//! which constitutes the core of the `keen-retry` Library API.
//! 
//! Here you will see the recommended Patterns for adding `keen-retry` support to your own library.
//! The code is organized as follows:
//!   1) Custom error definitions -- it is essential that the library is already designed in a way it becomes
//!      possible to distinguish between different errors. Also, for zero-copy, whenever an input is consumed
//!      but not used due to an error, it is important that the data gets back to the caller through one of
//!      the error variants.
//!   2) The original library methods -- these are totally agnostic of the `keen-retry` crate;
//!   3) The `keen-retry` wrappers -- these add variations over the original methods that enrich the
//!      standard Rust `Result<>` type into our `RetryResult<>`, able to distinguish transient errors.
//!   4) The Composable retry logic policies in the library level, show casing the separation of concerns

use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering::Relaxed},
        Arc,
    },
};
use log::{error, info, warn};

use keen_retry::{RetryConsumerResult, RetryProcedureResult, RetryProducerResult, RetryResult};
use thiserror::Error;


// 1) CUSTOM ERROR DEFINITIONS
// ///////////////////////////
// Notice that it is essential for the library to be designed in a way it becomes
// possible to distinguish between different errors. Also, for zero-copy, whenever an input is consumed
// (but not used due to an error), it is important that the data gets back to the caller through one of
// the error variants -- this way no copying is needed and, more importantly, the input doesn't need to
// be produced again.

pub type StdErrorType = Box<dyn std::error::Error + Send + Sync>;

/// Custom error types when connecting.\
/// Custom error types goes hand-in-hand with the `keen-retry` library (to distinguish among fatal and transient errors).
/// Consider using the `thiserror` crate if you prefer to log custom error messages instead of the error name.
#[derive(Error, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionErrors {
    /// A fatal error -- no metter how many times we try
    #[error("Wrong Credentials.")]
    WrongCredentials,
    /// A transient error -- trying again may yield different results
    #[error("Server too busy. Try again later.")]
    ServerTooBusy,
}
impl ConnectionErrors {
    /// The knowledge of which errors are fatal or transient may be better placed near the errors themselves,
    /// as a production application may have several such types.
    pub fn is_fatal(&self) -> bool {
        match self {
            ConnectionErrors::WrongCredentials => true,
            ConnectionErrors::ServerTooBusy    => false,
        }
    }
}

/// Custom error type when sending.\
/// Here you'll see how to work with errors that carry the `payload` of failed consumer operations,
/// leveraging zero-copy & removing the need for regenerating it on subsequent retries
/// -- the payload is wrapped in an `Option<>` as it needs to be owned, back and forth, by the `keen-retry` logic.
#[derive(Error, Debug)]
pub enum TransportErrors<Payload: Debug + PartialEq> {
    /// A fatal error (while sending)
    #[error("The sending quota has exhausted for the day.")]
    QuotaExhausted { payload: Option<Payload>, root_cause: StdErrorType },
    /// A transient error (becomes fatal if the number of retry attempts gets exhausted)
    #[error("The connection was dropped.")]
    ConnectionDropped { payload: Option<Payload>, root_cause: StdErrorType },
    /// A fatal error (while retrying a failed reconnection -- prevented nested retries from scaling up indefinitely)
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
    /// Extracts the payload from the given error
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
    /// Applies the given fn `f()` to the payload
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


/// Our fictitius library is a simple network client with `connect()`, `send()`, and `receive()` operations.\
/// We simulate errors for `connect()` and `send()`, through some latch counters:
///   a) No errors happen until `*_success_latch_countdown` reaches 0 -- which is decremented on each call;
///   b) While `*_success_latch_countdown` doesn't reach 0 and while `*_fatal_failure_latch_countdown` is > 0 (which
///      is also decremented on each call), a Transient error will be issued
///   c) Having `*_fatal_failure_latch_countdown` reached 0, only fatal errors will be issued.
pub struct Socket {
    is_connected:                             AtomicBool,
    /// number of calls for a successful connection to happen (if no failures as scheduled to happen after that, all further connection attempts will also be successful)
    connection_success_latch_countdown:       AtomicI32,
    connection_fatal_failure_latch_countdown: AtomicI32,
    connection_attempts:                      AtomicU32,
    sending_success_latch_countdown:          AtomicI32,
    sending_fatal_failure_latch_countdown:    AtomicI32,
    sending_attempts:                         AtomicU32,
}

impl Socket {

    pub fn new(connection_success_latch_countdown:       i32,
               connection_fatal_failure_latch_countdown: i32,
               sending_success_latch_countdown:          i32,
               sending_fatal_failure_latch_countdown:    i32) -> Arc<Self> {
        Arc::new(Self {
            is_connected:                             AtomicBool::new(false),
            connection_success_latch_countdown:       AtomicI32::new(connection_success_latch_countdown),
            connection_fatal_failure_latch_countdown: AtomicI32::new(connection_fatal_failure_latch_countdown),
            connection_attempts:                      AtomicU32::new(0),
            sending_success_latch_countdown:          AtomicI32::new(sending_success_latch_countdown),
            sending_fatal_failure_latch_countdown:    AtomicI32::new(sending_fatal_failure_latch_countdown),
            sending_attempts:                         AtomicU32::new(0),
         })
    }


    // 2) THE ORIGINAL LIBRARY METHODS
    // ///////////////////////////////
    // Your original methods, agnostic of the `keen-retry` crate

    /// Simulates a real connection, failing according to the configuration
    pub(crate) async fn connect_to_server_raw(&self) -> Result<(), ConnectionErrors> {
        self.connection_attempts.fetch_add(1, Relaxed);
        if self.connection_success_latch_countdown.fetch_sub(1, Relaxed) <= 1 {
            self.connection_fatal_failure_latch_countdown.fetch_sub(1, Relaxed);
            self.is_connected.store(true, Relaxed);
            Ok(())
        } else if self.connection_fatal_failure_latch_countdown.fetch_sub(1, Relaxed) <= 1 {
            self.is_connected.store(false, Relaxed);
            Err(ConnectionErrors::WrongCredentials)
        } else {
            self.is_connected.store(false, Relaxed);
            Err(ConnectionErrors::ServerTooBusy)
        }
    }

    /// Tells if we are connected or not
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Relaxed)
    }

    /// Simulates a real send, failing according to the configuration
    pub(crate) fn send_raw<T: Debug + PartialEq>
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
                Err(TransportErrors::QuotaExhausted { payload: Some(payload), root_cause: String::from("You abused sending. Please don't try to send anything else today or you will be banned!").into() })
            } else {
                self.is_connected.store(false, Relaxed);
                Err(TransportErrors::ConnectionDropped { payload: Some(payload), root_cause: String::from("Socket read error (-1)").into() })
            }
        }
    }

    /// This one doesn't fail extensively... all possible retries & recoveries are already tested in connect() & send().\
    /// Here, we are just interested into exploring the API over a function that produces results, instead of consuming it (like send() does)
    pub(crate) fn receive_raw(&self) -> Result<&'static str, TransportErrors<&'static str>> {
        if !self.is_connected() {
            Err(TransportErrors::NotConnected { payload: None })
        } else {
            Ok("'Hello, Earthling!'")
        }
    }



    // 3) The `keen-retry` wrappers
    // ////////////////////////////
    // Simply maps the standard `Result<>` types used in (1) to [crate::RetryResult]

    /// Wrapper around [Self::connect_to_server_raw()], enabling `keen-retry` on it
    pub async fn connect_to_server(&self) -> RetryProcedureResult<ConnectionErrors> {
        self.connect_to_server_raw().await
            .map_or_else(|error| match error.is_fatal() {
                            true  => RetryProcedureResult::Fatal     { input: (), error },
                            false => RetryProcedureResult::Transient { input: (), error },
                         },
                         |_| RetryProcedureResult::Ok { reported_input: (), output: () })
    }

    /// Wrapper around [Self::send_raw()], enabling `keen-retry` on it
    /// -- but not supposed to be public in this demonstration, as the public
    /// method [Self::send()] adds a small retry logic layer to show off the
    /// composition of nested retry policies.
    pub(crate) fn send_retryable<T: Debug + PartialEq>
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
                                 RetryConsumerResult::Transient { input: payload, error }
                             },
                         },
                         |_| RetryConsumerResult::Ok { reported_input: (), output: () })
    }

    /// Wrapper around [Self::receive()], enabling `keen-retry` on it
    pub fn receive(&self) -> RetryProducerResult<&'static str, TransportErrors<&'static str>> {

        self.receive_raw()
            .map_or_else(|error| match error.is_fatal() {
                             true  => RetryProducerResult::Fatal     { input: (), error },
                             false => RetryProducerResult::Transient { input: (), error },
                         },
                         |payload| RetryProducerResult::Ok  { reported_input: (), output: payload })
    }


    // 4) The `keen-retry` composable retry policies at the library level
    // //////////////////////////////////////////////////////////////////
    // They extend the normal `keen-retry` library integration with portions
    // of a retry strategy using the `.or_else()` high order function

    /// Wrapper around [Self::send_raw()], enabling `keen-retry` on it and adding a layer
    /// to the retrying process: check if the connection is ok and attempt to reconnect.\
    /// This wrapper demonstrates how to add the composable retry policy in the Library level
    /// -- for a composable retry policy in the application level, see [use_cases::broadcast()].
    pub async fn send<T: Debug + PartialEq>
                     (&self,
                      payload: T)
                     -> RetryConsumerResult<(), T, TransportErrors<T>> {

        self.send_retryable(payload)
            .or_else_with_async(|payload, error| async {
                if !self.is_connected() {
                    match self.connect_to_server().await {
                        RetryResult::Transient { input: _, error } => {
                            warn!("## `external_lib::send({payload:?})`: Transient failure attempting to reconnect: {}", error);
                        },
                        RetryResult::Fatal { input: _, error } => {
                            error!("## `external_lib::send({payload:?})`: Error attempting to reconnect (won't retry): {}", error);
                            return RetryResult::Fatal { input: payload, error: TransportErrors::CannotReconnect { payload: None, root_cause: error.into() } };
                        },
                        _ => {
                            info!("## `external_lib::send({payload:?})`: Reconnection succeeded after retrying");
                        },
                    }
                }
                RetryResult::Transient { input: payload, error }
            })
            .await
    }



    
}