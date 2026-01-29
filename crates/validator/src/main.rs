//! Snapshot validation tool.
//!
//! Validates KV store snapshots for:
//! - File integrity (header, checksum)
//! - Key-value consistency
//! - Lookup correctness
//!
//! # Usage
//!
//! ```bash
//! # Basic validation
//! validator --snapshot /data/snapshot.bin
//!
//! # Validate with expected key-value pairs
//! validator --snapshot /data/snapshot.bin --expected keys_values.json
//! ```

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use kv_store::{KvStore, KEY_SIZE, VALUE_SIZE};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "validator")]
#[command(about = "Validate KV store snapshots")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate snapshot integrity and perform random lookups
    Check {
        /// Path to the snapshot file
        #[arg(short, long)]
        snapshot: PathBuf,

        /// Number of random lookups to verify
        #[arg(long, default_value = "10000")]
        sample_size: usize,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Validate specific key-value pairs from a file
    Verify {
        /// Path to the snapshot file
        #[arg(short, long)]
        snapshot: PathBuf,

        /// Path to JSON file with expected key-value pairs
        #[arg(short, long)]
        expected: PathBuf,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Benchmark lookup performance
    Bench {
        /// Path to the snapshot file
        #[arg(short, long)]
        snapshot: PathBuf,

        /// Number of lookups to perform
        #[arg(long, default_value = "1000000")]
        iterations: usize,

        /// Number of warmup iterations
        #[arg(long, default_value = "10000")]
        warmup: usize,
    },

    /// Compare two snapshots for differences
    Diff {
        /// First snapshot file
        #[arg(short = 'a', long)]
        snapshot_a: PathBuf,

        /// Second snapshot file
        #[arg(short = 'b', long)]
        snapshot_b: PathBuf,

        /// Maximum differences to report
        #[arg(long, default_value = "100")]
        max_diffs: usize,
    },
}

#[derive(Serialize)]
struct ValidationResult {
    snapshot: String,
    record_count: usize,
    sample_size: usize,
    lookups_passed: usize,
    lookups_failed: usize,
    elapsed_ms: u128,
    status: String,
}

#[derive(Deserialize)]
struct ExpectedPair {
    key: String, // hex-encoded
    value: String, // hex-encoded
}

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Check {
            snapshot,
            sample_size,
            json,
        } => check_snapshot(snapshot, sample_size, json),

        Commands::Verify {
            snapshot,
            expected,
            json,
        } => verify_expected(snapshot, expected, json),

        Commands::Bench {
            snapshot,
            iterations,
            warmup,
        } => benchmark_lookups(snapshot, iterations, warmup),

        Commands::Diff {
            snapshot_a,
            snapshot_b,
            max_diffs,
        } => diff_snapshots(snapshot_a, snapshot_b, max_diffs),
    }
}

fn check_snapshot(snapshot: PathBuf, sample_size: usize, json: bool) -> Result<()> {
    let start = Instant::now();

    if !json {
        info!(snapshot = %snapshot.display(), "Opening snapshot");
    }

    let store = KvStore::open(&snapshot).context("Failed to open snapshot")?;

    if !json {
        info!(record_count = store.len(), "Snapshot loaded");
    }

    // Collect keys for sampling
    let keys: Vec<_> = store.iter().map(|(k, _)| *k).collect();

    // Sample random keys
    let mut rng = rand::thread_rng();
    let sample: Vec<_> = keys
        .choose_multiple(&mut rng, sample_size.min(keys.len()))
        .cloned()
        .collect();

    if !json {
        info!(sample_size = sample.len(), "Verifying random sample");
    }

    let pb = if !json {
        let pb = ProgressBar::new(sample.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")
                .unwrap(),
        );
        Some(pb)
    } else {
        None
    };

    let mut passed = 0;
    let mut failed = 0;

    for key in &sample {
        if store.get(key).is_some() {
            passed += 1;
        } else {
            failed += 1;
            if !json {
                warn!(key = ?&key[0..8], "Lookup failed for known key");
            }
        }

        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    let elapsed = start.elapsed();

    let result = ValidationResult {
        snapshot: snapshot.display().to_string(),
        record_count: store.len(),
        sample_size: sample.len(),
        lookups_passed: passed,
        lookups_failed: failed,
        elapsed_ms: elapsed.as_millis(),
        status: if failed == 0 { "PASS".into() } else { "FAIL".into() },
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!();
        println!("Validation Result");
        println!("=================");
        println!("Snapshot: {}", result.snapshot);
        println!("Record count: {}", result.record_count);
        println!("Sample size: {}", result.sample_size);
        println!("Lookups passed: {}", result.lookups_passed);
        println!("Lookups failed: {}", result.lookups_failed);
        println!("Elapsed: {}ms", result.elapsed_ms);
        println!("Status: {}", result.status);
    }

    if failed > 0 {
        anyhow::bail!("{} lookups failed", failed);
    }

    Ok(())
}

fn verify_expected(snapshot: PathBuf, expected: PathBuf, json: bool) -> Result<()> {
    info!(snapshot = %snapshot.display(), "Opening snapshot");
    let store = KvStore::open(&snapshot).context("Failed to open snapshot")?;

    info!(expected = %expected.display(), "Loading expected values");
    let file = File::open(&expected).context("Failed to open expected file")?;
    let pairs: Vec<ExpectedPair> = serde_json::from_reader(file)
        .context("Failed to parse expected file")?;

    info!(pairs = pairs.len(), "Verifying expected pairs");

    let pb = ProgressBar::new(pairs.len() as u64);
    let mut passed = 0;
    let mut failed = 0;

    for pair in &pairs {
        let key = hex_decode(&pair.key)?;
        let expected_value = hex_decode(&pair.value)?;

        if key.len() != KEY_SIZE {
            warn!(key_len = key.len(), "Invalid key size, skipping");
            continue;
        }

        let key: [u8; KEY_SIZE] = key.try_into().unwrap();

        match store.get(&key) {
            Some(value) if value[..expected_value.len()] == expected_value[..] => {
                passed += 1;
            }
            Some(value) => {
                failed += 1;
                if !json {
                    warn!(
                        key = pair.key,
                        expected = pair.value,
                        actual = hex_encode(&value[..expected_value.len()]),
                        "Value mismatch"
                    );
                }
            }
            None => {
                failed += 1;
                if !json {
                    warn!(key = pair.key, "Key not found");
                }
            }
        }

        pb.inc(1);
    }

    pb.finish();

    println!();
    println!("Verification Result: {} passed, {} failed", passed, failed);

    if failed > 0 {
        anyhow::bail!("{} verifications failed", failed);
    }

    Ok(())
}

fn benchmark_lookups(snapshot: PathBuf, iterations: usize, warmup: usize) -> Result<()> {
    info!(snapshot = %snapshot.display(), "Opening snapshot");
    let store = KvStore::open(&snapshot).context("Failed to open snapshot")?;

    info!("Preloading snapshot into RAM");
    store.preload()?;

    // Collect keys for lookup
    let keys: Vec<_> = store.iter().map(|(k, _)| *k).collect();

    if keys.is_empty() {
        anyhow::bail!("Snapshot is empty");
    }

    info!(warmup, "Running warmup iterations");
    for i in 0..warmup {
        let key = &keys[i % keys.len()];
        std::hint::black_box(store.get(key));
    }

    info!(iterations, "Running benchmark");

    let start = Instant::now();
    for i in 0..iterations {
        let key = &keys[i % keys.len()];
        std::hint::black_box(store.get(key));
    }
    let elapsed = start.elapsed();

    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();
    let ns_per_op = elapsed.as_nanos() as f64 / iterations as f64;

    println!();
    println!("Benchmark Results");
    println!("=================");
    println!("Iterations: {}", iterations);
    println!("Total time: {:.2}s", elapsed.as_secs_f64());
    println!("Ops/sec: {:.0}", ops_per_sec);
    println!("Latency: {:.0}ns/op ({:.2}μs/op)", ns_per_op, ns_per_op / 1000.0);

    Ok(())
}

fn diff_snapshots(snapshot_a: PathBuf, snapshot_b: PathBuf, max_diffs: usize) -> Result<()> {
    info!(a = %snapshot_a.display(), b = %snapshot_b.display(), "Comparing snapshots");

    let store_a = KvStore::open(&snapshot_a).context("Failed to open snapshot A")?;
    let store_b = KvStore::open(&snapshot_b).context("Failed to open snapshot B")?;

    println!("Snapshot A: {} records", store_a.len());
    println!("Snapshot B: {} records", store_b.len());

    let mut in_a_only = 0;
    let mut in_b_only = 0;
    let mut value_differs = 0;
    let mut diffs_shown = 0;

    // Check all keys in A
    for (key, value_a) in store_a.iter() {
        match store_b.get(key) {
            Some(value_b) if value_a != value_b => {
                value_differs += 1;
                if diffs_shown < max_diffs {
                    println!("VALUE_DIFF: key={}", hex_encode(&key[..16]));
                    diffs_shown += 1;
                }
            }
            None => {
                in_a_only += 1;
                if diffs_shown < max_diffs {
                    println!("IN_A_ONLY: key={}", hex_encode(&key[..16]));
                    diffs_shown += 1;
                }
            }
            _ => {}
        }
    }

    // Check for keys only in B
    for (key, _) in store_b.iter() {
        if store_a.get(key).is_none() {
            in_b_only += 1;
            if diffs_shown < max_diffs {
                println!("IN_B_ONLY: key={}", hex_encode(&key[..16]));
                diffs_shown += 1;
            }
        }
    }

    println!();
    println!("Summary");
    println!("=======");
    println!("In A only: {}", in_a_only);
    println!("In B only: {}", in_b_only);
    println!("Value differs: {}", value_differs);
    println!("Total differences: {}", in_a_only + in_b_only + value_differs);

    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .context("Invalid hex string")
}
