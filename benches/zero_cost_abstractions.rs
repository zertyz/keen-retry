//! This benchmark compares the performance of retryable operations using `keen-retry` against standard operations using `Result<>`.
//! The goal is to demonstrate that `keen-retry` introduces no additional runtime cost in scenarios where operations
//! either succeed or fail fatally on the first attempt.
//!
//! Reminder: `keen-retry` introduces a [RetryResult::Transient] variant for retryable operations, but this benchmark focuses on the
//! `Ok` and `Fatal` outcomes, which are directly comparable to the standard Rust `Result<>`. Since `Transient` implies
//! additional processing for retries, it falls outside the scope of this zero-cost comparison.
//!
//! The instrumentation provided here is only applicable to `Transient` results -- for further equivalence with the std::Result<>
//! benchmark. As such, it should not impact the benchmark when `Transient` is not present, allowing compile-time optimizations
//! to eliminate any unreachable code.
//!
//! # Analysis (2023-11-12)
//!
//! By annotating all `keen-retry` methods with `#[inline(always)]`, and applying the same to methods that utilize the
//! "Library API" and "Application API", we can achieve zero-cost abstractions for cases where operations are successful
//! or fail fatally on the first attempt. This is due to the compiler's ability to optimize inlined code.
//!
//! Therefore, when defining a library function -- see [operation()] -- or application-specific retry logic --
//! see [keen_operation()], it is recommended to use `#[inline(always)]` for critical performance paths. However, this
//! should be approached with caution as excessive inlining can lead to increased binary sizes.
//!
//! The benchmark assumes that the [RetryResult::Transient] state does not occur, focusing on comparing the overhead of
//! using `keen-retry` in the most common case where operations do not involve retry logic.
//!
//! The following code provides the setup and execution of the benchmarks, assuming a controlled environment where external
//! factors are minimized to ensure a fair comparison.
//!


use criterion::{criterion_group, criterion_main, Criterion, black_box};
use keen_retry::{RetryProducerResult, RetryResult};


/// Simulates an operation that alternates between `Ok` and `Err` variants.
/// The counter is used to simulate state changes in the operation.
#[inline(always)]
fn raw_operation() -> Result<u32, u32> {
    static mut COUNTER: u32 = 0;
    unsafe {
        COUNTER += 1;
        if COUNTER % 2 == 0 {
            Err(COUNTER)
        } else {
            Ok(COUNTER)
        }
    }
}

/// Wraps [raw_operation()] in `keen-retry`'s Library API, converting the
/// standard `Result` into a [RetryResult].
#[inline(always)]
fn operation() -> RetryProducerResult<u32, u32> {
    match raw_operation() {
        Ok(data) => RetryResult::Ok { reported_input: (), output: data },
        Err(data) => RetryResult::Fatal { input: (), error: data },
    }
}

/// Applies `keen-retry`'s Application API to the retryable operation, adding
/// instrumentation and retry logic. Since this benchmark's [operation()] does not
/// produce [RetryResult::Transient] results, no retries or instrumentation will be invoked.
#[inline(always)]
fn keen_operation() -> Result<u32, u32> {
    operation()
        .inspect_transient(|_, _| println!("Logs that a set of retries will start due to a transient error -- not happening in this benchmark"))
        .retry_with(|_| operation())
        .spinning_forever()
        .inspect_recovered(|_, _, _| println!("Logs that the retry process recovered from the failure and the operation succeeded -- this state is not happening in this benchmark"))
        .inspect_given_up(|_, _, _| println!("Logs that a all retry attempts were exhausted without success -- not happening in this benchmark"))
        .into_result()
}


/// Benchmarks comparing the performance of standard Rust `Result<>` to `keen-retry` results [RetryResult] and [keen_retry::ResolvedResult].
fn bench_zero_cost_abstractions(criterion: &mut Criterion) {

    let mut group = criterion.benchmark_group("Zero Cost Abstractions");

    let bench_id = "std::Result<>";
    group.bench_function(bench_id, |bencher| bencher.iter(|| {
        black_box({
            _ = raw_operation();
        })
    }));

    let bench_id = "keen_retry::RetryResult<> -- when opting-out from the retry process";
    group.bench_function(bench_id, |bencher| bencher.iter(|| {
        black_box({
            _ = operation().into_result();
        })
    }));

    let bench_id = "keen_retry::ResolvedResult<> -- when opting-in to the retry process";
    group.bench_function(bench_id, |bencher| bencher.iter(|| {
        black_box({
            _ = keen_operation();
        })
    }));

    group.finish();
}


criterion_group!(benches, bench_zero_cost_abstractions);
criterion_main!(benches);