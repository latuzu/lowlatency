//! Prometheus metrics for the KV server.

use prometheus::{
    Counter, Encoder, Histogram, HistogramOpts, IntCounter, IntCounterVec, Opts, Registry,
    TextEncoder,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Prometheus metrics for monitoring KV server performance.
pub struct Metrics {
    registry: Registry,
    /// Total number of requests
    requests_total: IntCounter,
    /// Number of cache hits (key found)
    hits_total: IntCounter,
    /// Number of cache misses (key not found)
    misses_total: IntCounter,
    /// Number of errors
    errors_total: IntCounter,
    /// Request latency histogram
    latency: Histogram,
}

impl Metrics {
    /// Creates a new metrics instance with default configuration.
    pub fn new() -> Self {
        let registry = Registry::new();

        let requests_total = IntCounter::with_opts(Opts::new(
            "kv_requests_total",
            "Total number of KV requests",
        ))
        .unwrap();

        let hits_total = IntCounter::with_opts(Opts::new(
            "kv_hits_total",
            "Total number of cache hits",
        ))
        .unwrap();

        let misses_total = IntCounter::with_opts(Opts::new(
            "kv_misses_total",
            "Total number of cache misses",
        ))
        .unwrap();

        let errors_total = IntCounter::with_opts(Opts::new(
            "kv_errors_total",
            "Total number of errors",
        ))
        .unwrap();

        // Latency buckets optimized for our target (p99 < 5ms)
        // Buckets in seconds: 10us, 50us, 100us, 500us, 1ms, 2ms, 5ms, 10ms, 50ms, 100ms
        let latency = Histogram::with_opts(
            HistogramOpts::new("kv_request_latency_seconds", "Request latency in seconds")
                .buckets(vec![
                    0.00001,  // 10μs
                    0.00005,  // 50μs
                    0.0001,   // 100μs
                    0.0005,   // 500μs
                    0.001,    // 1ms
                    0.002,    // 2ms
                    0.005,    // 5ms (our target)
                    0.01,     // 10ms
                    0.05,     // 50ms
                    0.1,      // 100ms
                ]),
        )
        .unwrap();

        // Register all metrics
        registry.register(Box::new(requests_total.clone())).unwrap();
        registry.register(Box::new(hits_total.clone())).unwrap();
        registry.register(Box::new(misses_total.clone())).unwrap();
        registry.register(Box::new(errors_total.clone())).unwrap();
        registry.register(Box::new(latency.clone())).unwrap();

        Self {
            registry,
            requests_total,
            hits_total,
            misses_total,
            errors_total,
            latency,
        }
    }

    /// Increments the total request counter.
    #[inline]
    pub fn increment_requests(&self) {
        self.requests_total.inc();
    }

    /// Increments the cache hit counter.
    #[inline]
    pub fn increment_hits(&self) {
        self.hits_total.inc();
    }

    /// Increments the cache miss counter.
    #[inline]
    pub fn increment_misses(&self) {
        self.misses_total.inc();
    }

    /// Increments the error counter.
    #[inline]
    pub fn increment_errors(&self) {
        self.errors_total.inc();
    }

    /// Records a request latency.
    #[inline]
    pub fn record_latency(&self, duration: Duration) {
        self.latency.observe(duration.as_secs_f64());
    }

    /// Encodes all metrics to Prometheus text format.
    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_encode() {
        let metrics = Metrics::new();
        metrics.increment_requests();
        metrics.increment_hits();
        metrics.record_latency(Duration::from_micros(100));

        let encoded = metrics.encode();
        assert!(encoded.contains("kv_requests_total"));
        assert!(encoded.contains("kv_hits_total"));
        assert!(encoded.contains("kv_request_latency_seconds"));
    }
}
