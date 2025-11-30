//! Metrics Collection Infrastructure
//!
//! Provides comprehensive metrics collection including:
//! - Latency percentiles (p50, p90, p99)
//! - Error rates by operation type
//! - Cache hit ratios
//! - Request throughput

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// A metric label key-value pair
pub type Label = (String, String);

/// Types of metrics we can collect
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

/// A counter metric that only goes up
#[derive(Debug)]
pub struct Counter {
    value: AtomicU64,
    labels: Vec<Label>,
}

impl Counter {
    pub fn new(labels: Vec<Label>) -> Self {
        Self {
            value: AtomicU64::new(0),
            labels,
        }
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }
}

/// A gauge metric that can go up or down
#[derive(Debug)]
pub struct Gauge {
    value: AtomicU64,
    labels: Vec<Label>,
}

impl Gauge {
    pub fn new(labels: Vec<Label>) -> Self {
        Self {
            value: AtomicU64::new(0),
            labels,
        }
    }

    pub fn set(&self, value: u64) {
        self.value.store(value, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }
}

/// A histogram for measuring distributions (latencies, sizes, etc.)
#[derive(Debug)]
pub struct Histogram {
    buckets: Vec<(f64, AtomicU64)>,
    sum: AtomicU64, // Sum stored as micros (u64)
    count: AtomicU64,
    labels: Vec<Label>,
}

impl Histogram {
    /// Create a histogram with default latency buckets (in seconds)
    pub fn new(labels: Vec<Label>) -> Self {
        Self::with_buckets(
            labels,
            vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
        )
    }

    /// Create a histogram with custom bucket boundaries
    pub fn with_buckets(labels: Vec<Label>, bucket_bounds: Vec<f64>) -> Self {
        let buckets = bucket_bounds
            .into_iter()
            .map(|b| (b, AtomicU64::new(0)))
            .collect();
        Self {
            buckets,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
            labels,
        }
    }

    /// Observe a value
    pub fn observe(&self, value: f64) {
        // Increment the appropriate bucket(s)
        for (bound, count) in &self.buckets {
            if value <= *bound {
                count.fetch_add(1, Ordering::Relaxed);
            }
        }
        // Update sum and count
        let micros = (value * 1_000_000.0) as u64;
        self.sum.fetch_add(micros, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Observe a duration
    pub fn observe_duration(&self, duration: Duration) {
        self.observe(duration.as_secs_f64());
    }

    /// Get the count of observations
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get the sum of all observations
    pub fn sum(&self) -> f64 {
        self.sum.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// Calculate percentile (approximate using buckets)
    pub fn percentile(&self, p: f64) -> Option<f64> {
        let total = self.count.load(Ordering::Relaxed);
        if total == 0 {
            return None;
        }

        let target = (total as f64 * p / 100.0) as u64;
        let mut prev_bound = 0.0;
        let mut prev_count = 0u64;

        for (bound, count) in &self.buckets {
            let bucket_count = count.load(Ordering::Relaxed);
            if bucket_count >= target {
                // Linear interpolation within the bucket
                if bucket_count == prev_count {
                    return Some(*bound);
                }
                let fraction = (target - prev_count) as f64 / (bucket_count - prev_count) as f64;
                return Some(prev_bound + fraction * (*bound - prev_bound));
            }
            prev_bound = *bound;
            prev_count = bucket_count;
        }

        // Return the last bucket bound if we couldn't find it
        self.buckets.last().map(|(b, _)| *b)
    }

    /// Get p50, p90, p99 percentiles
    pub fn percentiles(&self) -> PercentileStats {
        PercentileStats {
            p50: self.percentile(50.0),
            p90: self.percentile(90.0),
            p95: self.percentile(95.0),
            p99: self.percentile(99.0),
        }
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }
}

/// Percentile statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PercentileStats {
    pub p50: Option<f64>,
    pub p90: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
}

/// A timer that automatically records duration when dropped
pub struct Timer<'a> {
    histogram: &'a Histogram,
    start: Instant,
}

impl<'a> Timer<'a> {
    pub fn new(histogram: &'a Histogram) -> Self {
        Self {
            histogram,
            start: Instant::now(),
        }
    }

    /// Get elapsed time without stopping
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl<'a> Drop for Timer<'a> {
    fn drop(&mut self) {
        self.histogram.observe_duration(self.start.elapsed());
    }
}

/// Central metrics registry
pub struct MetricsRegistry {
    counters: RwLock<HashMap<String, Arc<Counter>>>,
    gauges: RwLock<HashMap<String, Arc<Gauge>>>,
    histograms: RwLock<HashMap<String, Arc<Histogram>>>,
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            gauges: RwLock::new(HashMap::new()),
            histograms: RwLock::new(HashMap::new()),
        }
    }

    /// Get or create a counter
    pub fn counter(&self, name: &str, labels: Vec<Label>) -> Arc<Counter> {
        let key = Self::make_key(name, &labels);
        {
            let counters = self.counters.read();
            if let Some(counter) = counters.get(&key) {
                return counter.clone();
            }
        }
        let mut counters = self.counters.write();
        counters
            .entry(key)
            .or_insert_with(|| Arc::new(Counter::new(labels)))
            .clone()
    }

    /// Get or create a gauge
    pub fn gauge(&self, name: &str, labels: Vec<Label>) -> Arc<Gauge> {
        let key = Self::make_key(name, &labels);
        {
            let gauges = self.gauges.read();
            if let Some(gauge) = gauges.get(&key) {
                return gauge.clone();
            }
        }
        let mut gauges = self.gauges.write();
        gauges
            .entry(key)
            .or_insert_with(|| Arc::new(Gauge::new(labels)))
            .clone()
    }

    /// Get or create a histogram
    pub fn histogram(&self, name: &str, labels: Vec<Label>) -> Arc<Histogram> {
        let key = Self::make_key(name, &labels);
        {
            let histograms = self.histograms.read();
            if let Some(histogram) = histograms.get(&key) {
                return histogram.clone();
            }
        }
        let mut histograms = self.histograms.write();
        histograms
            .entry(key)
            .or_insert_with(|| Arc::new(Histogram::new(labels)))
            .clone()
    }

    fn make_key(name: &str, labels: &[Label]) -> String {
        let mut key = name.to_string();
        for (k, v) in labels {
            key.push_str(&format!("|{}={}", k, v));
        }
        key
    }

    /// Export all metrics as a snapshot
    pub fn snapshot(&self) -> MetricsSnapshot {
        let counters = self.counters.read();
        let gauges = self.gauges.read();
        let histograms = self.histograms.read();

        MetricsSnapshot {
            counters: counters
                .iter()
                .map(|(k, v)| (k.clone(), v.get()))
                .collect(),
            gauges: gauges
                .iter()
                .map(|(k, v)| (k.clone(), v.get()))
                .collect(),
            histograms: histograms
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        HistogramSnapshot {
                            count: v.count(),
                            sum: v.sum(),
                            percentiles: v.percentiles(),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// A snapshot of all metrics at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub counters: HashMap<String, u64>,
    pub gauges: HashMap<String, u64>,
    pub histograms: HashMap<String, HistogramSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramSnapshot {
    pub count: u64,
    pub sum: f64,
    pub percentiles: PercentileStats,
}

/// Pre-defined metrics for Glacis operations
pub struct GlacisMetrics {
    pub registry: Arc<MetricsRegistry>,

    // Request metrics
    pub requests_total: Arc<Counter>,
    pub requests_errors: Arc<Counter>,
    pub request_latency: Arc<Histogram>,

    // CTE operation metrics
    pub coverage_computations: Arc<Counter>,
    pub gap_analyses: Arc<Counter>,
    pub conflict_resolutions: Arc<Counter>,

    // Cache metrics
    pub cache_hits: Arc<Counter>,
    pub cache_misses: Arc<Counter>,

    // Receipt metrics
    pub receipts_minted: Arc<Counter>,
    pub receipts_verified: Arc<Counter>,

    // Active connections/sessions
    pub active_sessions: Arc<Gauge>,
}

impl Default for GlacisMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl GlacisMetrics {
    pub fn new() -> Self {
        let registry = Arc::new(MetricsRegistry::new());

        Self {
            requests_total: registry.counter("glacis_requests_total", vec![]),
            requests_errors: registry.counter("glacis_requests_errors_total", vec![]),
            request_latency: registry.histogram("glacis_request_latency_seconds", vec![]),
            coverage_computations: registry.counter("glacis_coverage_computations_total", vec![]),
            gap_analyses: registry.counter("glacis_gap_analyses_total", vec![]),
            conflict_resolutions: registry.counter("glacis_conflict_resolutions_total", vec![]),
            cache_hits: registry.counter("glacis_cache_hits_total", vec![]),
            cache_misses: registry.counter("glacis_cache_misses_total", vec![]),
            receipts_minted: registry.counter("glacis_receipts_minted_total", vec![]),
            receipts_verified: registry.counter("glacis_receipts_verified_total", vec![]),
            active_sessions: registry.gauge("glacis_active_sessions", vec![]),
            registry,
        }
    }

    /// Record a successful request
    pub fn record_request(&self, duration: Duration) {
        self.requests_total.inc();
        self.request_latency.observe_duration(duration);
    }

    /// Record an error
    pub fn record_error(&self) {
        self.requests_errors.inc();
    }

    /// Get the error rate (errors / total)
    pub fn error_rate(&self) -> f64 {
        let total = self.requests_total.get();
        if total == 0 {
            return 0.0;
        }
        self.requests_errors.get() as f64 / total as f64
    }

    /// Get the cache hit ratio
    pub fn cache_hit_ratio(&self) -> f64 {
        let hits = self.cache_hits.get();
        let misses = self.cache_misses.get();
        let total = hits + misses;
        if total == 0 {
            return 0.0;
        }
        hits as f64 / total as f64
    }

    /// Start a request timer
    pub fn start_request_timer(&self) -> Timer<'_> {
        Timer::new(&self.request_latency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let counter = Counter::new(vec![("operation".into(), "test".into())]);
        assert_eq!(counter.get(), 0);
        counter.inc();
        assert_eq!(counter.get(), 1);
        counter.inc_by(5);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new(vec![]);
        gauge.set(10);
        assert_eq!(gauge.get(), 10);
        gauge.inc();
        assert_eq!(gauge.get(), 11);
        gauge.dec();
        assert_eq!(gauge.get(), 10);
    }

    #[test]
    fn test_histogram() {
        let histogram = Histogram::new(vec![]);
        histogram.observe(0.05);
        histogram.observe(0.15);
        histogram.observe(0.50);
        histogram.observe(1.0);

        assert_eq!(histogram.count(), 4);
        assert!((histogram.sum() - 1.70).abs() < 0.01);

        let percentiles = histogram.percentiles();
        assert!(percentiles.p50.is_some());
    }

    #[test]
    fn test_metrics_registry() {
        let registry = MetricsRegistry::new();

        let counter1 = registry.counter("test_counter", vec![("env".into(), "prod".into())]);
        let counter2 = registry.counter("test_counter", vec![("env".into(), "prod".into())]);

        counter1.inc();
        assert_eq!(counter2.get(), 1); // Same counter
    }

    #[test]
    fn test_glacis_metrics() {
        let metrics = GlacisMetrics::new();

        metrics.record_request(Duration::from_millis(50));
        metrics.record_request(Duration::from_millis(100));
        metrics.record_error();

        assert_eq!(metrics.requests_total.get(), 2);
        assert_eq!(metrics.requests_errors.get(), 1);
        assert!((metrics.error_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_cache_hit_ratio() {
        let metrics = GlacisMetrics::new();

        metrics.cache_hits.inc_by(80);
        metrics.cache_misses.inc_by(20);

        assert!((metrics.cache_hit_ratio() - 0.8).abs() < 0.01);
    }
}
