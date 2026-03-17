use crate::infra::metrics::{
    record_circuit_breaker_open, record_circuit_breaker_rejected, set_circuit_breaker_state,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct CircuitBreaker {
    service_name: &'static str,
    failure_threshold: u32,
    open_duration: Duration,
    success_threshold: u32,
    failure_window: Duration,
    state: Arc<Mutex<CircuitBreakerState>>,
}

impl CircuitBreaker {
    pub fn new(
        service_name: &'static str,
        failure_threshold: u32,
        open_duration: Duration,
        success_threshold: u32,
        failure_window: Duration,
    ) -> Self {
        Self {
            service_name,
            failure_threshold,
            open_duration,
            success_threshold,
            failure_window,
            state: Arc::new(Mutex::new(CircuitBreakerState::Closed(ClosedState {
                consecutive_failures: 0,
                recent_outcomes: VecDeque::new(),
            }))),
        }
        .with_initialized_metrics()
    }

    fn with_initialized_metrics(self) -> Self {
        set_circuit_breaker_state(self.service_name, 0);
        self
    }

    pub fn allow_request(&self) -> Result<(), CircuitBreakerOpenError> {
        let mut state = self
            .state
            .lock()
            .expect("circuit breaker mutex must not be poisoned");

        match &mut *state {
            CircuitBreakerState::Closed(_) => Ok(()),
            CircuitBreakerState::Open { opened_at } => {
                let elapsed = opened_at.elapsed();

                if elapsed >= self.open_duration {
                    *state = CircuitBreakerState::HalfOpen {
                        consecutive_successes: 0,
                    };
                    set_circuit_breaker_state(self.service_name, 1);
                    tracing::warn!(
                        service = self.service_name,
                        success_threshold = self.success_threshold,
                        "circuit breaker moved to half-open"
                    );
                    Ok(())
                } else {
                    record_circuit_breaker_rejected(self.service_name);
                    Err(CircuitBreakerOpenError {
                        service_name: self.service_name,
                        remaining_open_seconds: self.open_duration.saturating_sub(elapsed).as_secs(),
                    })
                }
            }
            CircuitBreakerState::HalfOpen { .. } => Ok(()),
        }
    }

    pub fn record_success(&self) {
        let mut state = self
            .state
            .lock()
            .expect("circuit breaker mutex must not be poisoned");

        match &mut *state {
            CircuitBreakerState::Closed(closed) => {
                closed.consecutive_failures = 0;
                prune_outcomes(&mut closed.recent_outcomes, self.failure_window);
                closed.recent_outcomes.push_back(OutcomeRecord {
                    at: Instant::now(),
                    success: true,
                });

                if is_failure_rate_threshold_reached(
                    &closed.recent_outcomes,
                    self.failure_threshold as usize,
                ) {
                    *state = CircuitBreakerState::Open {
                        opened_at: Instant::now(),
                    };
                    record_circuit_breaker_open(self.service_name);
                    set_circuit_breaker_state(self.service_name, 2);
                    tracing::warn!(
                        service = self.service_name,
                        failure_threshold = self.failure_threshold,
                        failure_window_seconds = self.failure_window.as_secs(),
                        open_seconds = self.open_duration.as_secs(),
                        "circuit breaker opened"
                    );
                }
            }
            CircuitBreakerState::HalfOpen {
                consecutive_successes,
            } => {
                *consecutive_successes += 1;

                if *consecutive_successes >= self.success_threshold {
                    *state = CircuitBreakerState::Closed(ClosedState {
                        consecutive_failures: 0,
                        recent_outcomes: VecDeque::new(),
                    });
                    set_circuit_breaker_state(self.service_name, 0);
                    tracing::warn!(service = self.service_name, "circuit breaker closed");
                }
            }
            CircuitBreakerState::Open { .. } => {}
        }
    }

    pub fn record_failure(&self) {
        let mut state = self
            .state
            .lock()
            .expect("circuit breaker mutex must not be poisoned");

        match &mut *state {
            CircuitBreakerState::Closed(closed) => {
                closed.consecutive_failures += 1;
                prune_outcomes(&mut closed.recent_outcomes, self.failure_window);
                closed.recent_outcomes.push_back(OutcomeRecord {
                    at: Instant::now(),
                    success: false,
                });

                if closed.consecutive_failures >= self.failure_threshold
                    || is_failure_rate_threshold_reached(
                        &closed.recent_outcomes,
                        self.failure_threshold as usize,
                    )
                {
                    *state = CircuitBreakerState::Open {
                        opened_at: Instant::now(),
                    };
                    record_circuit_breaker_open(self.service_name);
                    set_circuit_breaker_state(self.service_name, 2);
                    tracing::warn!(
                        service = self.service_name,
                        failure_threshold = self.failure_threshold,
                        failure_window_seconds = self.failure_window.as_secs(),
                        open_seconds = self.open_duration.as_secs(),
                        "circuit breaker opened"
                    );
                }
            }
            CircuitBreakerState::HalfOpen { .. } => {
                *state = CircuitBreakerState::Open {
                    opened_at: Instant::now(),
                };
                record_circuit_breaker_open(self.service_name);
                set_circuit_breaker_state(self.service_name, 2);
                tracing::warn!(
                    service = self.service_name,
                    open_seconds = self.open_duration.as_secs(),
                    "circuit breaker reopened from half-open"
                );
            }
            CircuitBreakerState::Open { .. } => {}
        }
    }
}

#[derive(Clone)]
enum CircuitBreakerState {
    Closed(ClosedState),
    HalfOpen { consecutive_successes: u32 },
    Open { opened_at: Instant },
}

#[derive(Clone)]
struct ClosedState {
    consecutive_failures: u32,
    recent_outcomes: VecDeque<OutcomeRecord>,
}

#[derive(Clone, Copy)]
struct OutcomeRecord {
    at: Instant,
    success: bool,
}

fn prune_outcomes(outcomes: &mut VecDeque<OutcomeRecord>, failure_window: Duration) {
    let now = Instant::now();
    while let Some(front) = outcomes.front() {
        if now.duration_since(front.at) > failure_window {
            outcomes.pop_front();
        } else {
            break;
        }
    }
}

fn is_failure_rate_threshold_reached(
    outcomes: &VecDeque<OutcomeRecord>,
    minimum_samples: usize,
) -> bool {
    if outcomes.len() < minimum_samples {
        return false;
    }

    let failures = outcomes.iter().filter(|outcome| !outcome.success).count();
    failures * 2 > outcomes.len()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state_name(breaker: &CircuitBreaker) -> &'static str {
        let state = breaker
            .state
            .lock()
            .expect("circuit breaker mutex must not be poisoned");

        match &*state {
            CircuitBreakerState::Closed(_) => "closed",
            CircuitBreakerState::HalfOpen { .. } => "half_open",
            CircuitBreakerState::Open { .. } => "open",
        }
    }

    #[test]
    fn opens_when_failure_rate_exceeds_half_with_enough_samples() {
        let breaker = CircuitBreaker::new(
            "test_service",
            5,
            Duration::from_secs(30),
            3,
            Duration::from_secs(30),
        );

        breaker.record_failure();
        breaker.record_success();
        breaker.record_failure();
        breaker.record_failure();
        breaker.record_success();

        let error = breaker
            .allow_request()
            .expect_err("breaker should be open after >50% failures in window");
        assert!(error.to_string().contains("test_service circuit breaker is open"));
    }

    #[test]
    fn half_open_requires_success_threshold_to_close() {
        let breaker = CircuitBreaker::new(
            "test_service",
            1,
            Duration::from_millis(1),
            3,
            Duration::from_secs(30),
        );

        breaker.record_failure();
        assert_eq!(state_name(&breaker), "open");
        std::thread::sleep(Duration::from_millis(5));

        breaker
            .allow_request()
            .expect("breaker should move to half-open after cooldown");
        assert_eq!(state_name(&breaker), "half_open");
        breaker.record_success();
        assert_eq!(state_name(&breaker), "half_open");
        breaker.record_success();
        assert_eq!(state_name(&breaker), "half_open");

        breaker.record_success();
        assert_eq!(state_name(&breaker), "closed");
    }
}
