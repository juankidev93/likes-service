use crate::metrics::{
    record_circuit_breaker_open, record_circuit_breaker_rejected, set_circuit_breaker_state,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct CircuitBreaker {
    service_name: &'static str,
    failure_threshold: u32,
    open_duration: Duration,
    state: Arc<Mutex<CircuitBreakerState>>,
}

impl CircuitBreaker {
    pub fn new(
        service_name: &'static str,
        failure_threshold: u32,
        open_duration: Duration,
    ) -> Self {
        Self {
            service_name,
            failure_threshold,
            open_duration,
            state: Arc::new(Mutex::new(CircuitBreakerState::Closed {
                consecutive_failures: 0,
            })),
        }
        .with_initialized_metrics()
    }

    fn with_initialized_metrics(self) -> Self {
        set_circuit_breaker_state(self.service_name, false);
        self
    }

    pub fn allow_request(&self) -> Result<(), CircuitBreakerOpenError> {
        let mut state = self
            .state
            .lock()
            .expect("circuit breaker mutex must not be poisoned");

        match *state {
            CircuitBreakerState::Closed { .. } => Ok(()),
            CircuitBreakerState::Open { opened_at } => {
                let elapsed = opened_at.elapsed();

                if elapsed >= self.open_duration {
                    *state = CircuitBreakerState::Closed {
                        consecutive_failures: 0,
                    };
                    set_circuit_breaker_state(self.service_name, false);
                    Ok(())
                } else {
                    record_circuit_breaker_rejected(self.service_name);
                    Err(CircuitBreakerOpenError {
                        service_name: self.service_name,
                        remaining_open_seconds: self.open_duration.saturating_sub(elapsed).as_secs(),
                    })
                }
            }
        }
    }

    pub fn record_success(&self) {
        let mut state = self
            .state
            .lock()
            .expect("circuit breaker mutex must not be poisoned");

        *state = CircuitBreakerState::Closed {
            consecutive_failures: 0,
        };
        set_circuit_breaker_state(self.service_name, false);
    }

    pub fn record_failure(&self) {
        let mut state = self
            .state
            .lock()
            .expect("circuit breaker mutex must not be poisoned");

        match *state {
            CircuitBreakerState::Closed {
                ref mut consecutive_failures,
            } => {
                *consecutive_failures += 1;

                if *consecutive_failures >= self.failure_threshold {
                    *state = CircuitBreakerState::Open {
                        opened_at: Instant::now(),
                    };
                    record_circuit_breaker_open(self.service_name);
                    tracing::warn!(
                        service = self.service_name,
                        failure_threshold = self.failure_threshold,
                        open_seconds = self.open_duration.as_secs(),
                        "circuit breaker opened"
                    );
                }
            }
            CircuitBreakerState::Open { .. } => {}
        }
    }
}

#[derive(Clone, Copy)]
enum CircuitBreakerState {
    Closed { consecutive_failures: u32 },
    Open { opened_at: Instant },
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerOpenError {
    service_name: &'static str,
    remaining_open_seconds: u64,
}

impl std::fmt::Display for CircuitBreakerOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} circuit breaker is open for {} more seconds",
            self.service_name, self.remaining_open_seconds
        )
    }
}

impl std::error::Error for CircuitBreakerOpenError {}
