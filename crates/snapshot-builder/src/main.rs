//! Snapshot builder CLI for creating KV store snapshots.
//!
//! This tool builds snapshot files from various input formats (CSV, binary, random).
//!
//! # Usage
//!
//! ```bash
//! # Build from CSV
//! snapshot-builder --input data.csv --output snapshot.bin
//!
//! # Generate random test data
//! snapshot-builder --random --count 1000000 --output snapshot.bin
//! ```

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use kv_store::{Header, Mphf, Record, KEY_SIZE, RECORD_SIZE, VALUE_SIZE};
use memmap2::MmapMut;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

mod builder;

use builder::SnapshotBuilder;

#[derive(Parser)]
#[command(name = "snapshot-builder")]
#[command(about = "Build KV store snapshots from various data sources")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build snapshot from a CSV file
    Csv {
        /// Input CSV file path
        #[arg(short, long)]
        input: PathBuf,

        /// Output snapshot file path
        #[arg(short, long)]
        output: PathBuf,

        /// Column index for keys (0-based)
        #[arg(long, default_value = "0")]
        key_column: usize,

        /// Column index for values (0-based)
        #[arg(long, default_value = "1")]
        value_column: usize,

        /// Skip header row
        #[arg(long, default_value = "true")]
        has_header: bool,
    },

    /// Build snapshot from raw binary file (key-value pairs concatenated)
    Binary {
        /// Input binary file path
        #[arg(short, long)]
        input: PathBuf,

        /// Output snapshot file path
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Generate random test data
    Random {
        /// Output snapshot file path
        #[arg(short, long)]
        output: PathBuf,

        /// Number of records to generate
        #[arg(short, long)]
        count: usize,

        /// Random seed (optional, for reproducibility)
        #[arg(long)]
        seed: Option<u64>,
    },

    /// Validate an existing snapshot
    Validate {
        /// Snapshot file path
        #[arg(short, long)]
        snapshot: PathBuf,

        /// Number of random lookups to verify
        #[arg(long, default_value = "1000")]
        sample_size: usize,
    },

    /// Print snapshot metadata
    Info {
        /// Snapshot file path
        #[arg(short, long)]
        snapshot: PathBuf,
    },
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
        Commands::Csv {
            input,
            output,
            key_column,
            value_column,
            has_header,
        } => build_from_csv(input, output, key_column, value_column, has_header),

        Commands::Binary { input, output } => build_from_binary(input, output),

        Commands::Random {
            output,
            count,
            seed,
        } => build_random(output, count, seed),

        Commands::Validate {
            snapshot,
            sample_size,
        } => validate_snapshot(snapshot, sample_size),

        Commands::Info { snapshot } => print_info(snapshot),
    }
}

fn build_from_csv(
    input: PathBuf,
    output: PathBuf,
    key_column: usize,
    value_column: usize,
    has_header: bool,
) -> Result<()> {
    info!(input = %input.display(), "Reading CSV file");

    let file = File::open(&input).context("Failed to open input file")?;
    let reader = BufReader::new(file);
    let mut csv_reader = csv::ReaderBuilder::new()
        .has_headers(has_header)
        .from_reader(reader);

    let mut builder = SnapshotBuilder::new();

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap(),
    );

    let mut count = 0;
    for result in csv_reader.records() {
        let record = result.context("Failed to parse CSV record")?;

        let key_str = record
            .get(key_column)
            .context("Key column not found")?;
        let value_str = record
            .get(value_column)
            .context("Value column not found")?;

        // Convert string to fixed-size arrays (pad or truncate)
        let key = string_to_key(key_str);
        let value = string_to_value(value_str);

        builder.add_record(key, value);
        count += 1;

        if count % 100_000 == 0 {
            pb.set_message(format!("Read {} records", count));
        }
    }

    pb.finish_with_message(format!("Read {} records total", count));

    info!(count, "Building snapshot");
    builder.build(&output)?;

    info!(output = %output.display(), "Snapshot created successfully");
    Ok(())
}

fn build_from_binary(input: PathBuf, output: PathBuf) -> Result<()> {
    info!(input = %input.display(), "Reading binary file");

    let file = File::open(&input).context("Failed to open input file")?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };

    if mmap.len() % RECORD_SIZE != 0 {
        anyhow::bail!(
            "Binary file size {} is not a multiple of record size {}",
            mmap.len(),
            RECORD_SIZE
        );
    }

    let record_count = mmap.len() / RECORD_SIZE;
    info!(record_count, "Found records in binary file");

    let mut builder = SnapshotBuilder::new();

    let pb = ProgressBar::new(record_count as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    for i in 0..record_count {
        let offset = i * RECORD_SIZE;
        let key: [u8; KEY_SIZE] = mmap[offset..offset + KEY_SIZE].try_into().unwrap();
        let value: [u8; VALUE_SIZE] = mmap[offset + KEY_SIZE..offset + RECORD_SIZE]
            .try_into()
            .unwrap();

        builder.add_record(key, value);
        pb.inc(1);
    }

    pb.finish_with_message("Read complete");

    info!(record_count, "Building snapshot");
    builder.build(&output)?;

    info!(output = %output.display(), "Snapshot created successfully");
    Ok(())
}

fn build_random(output: PathBuf, count: usize, seed: Option<u64>) -> Result<()> {
    use rand::{Rng, SeedableRng};
    use rand::rngs::StdRng;

    info!(count, seed = ?seed, "Generating random test data");

    let mut rng = match seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_entropy(),
    };

    let mut builder = SnapshotBuilder::new();

    let pb = ProgressBar::new(count as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    for i in 0..count {
        let mut key = [0u8; KEY_SIZE];
        let mut value = [0u8; VALUE_SIZE];

        // Use index as prefix for deterministic keys (helps with testing)
        key[0..8].copy_from_slice(&(i as u64).to_le_bytes());
        rng.fill(&mut key[8..]);
        rng.fill(&mut value[..]);

        builder.add_record(key, value);
        pb.inc(1);
    }

    pb.finish_with_message("Generation complete");

    info!(count, "Building snapshot");
    builder.build(&output)?;

    info!(output = %output.display(), "Snapshot created successfully");
    Ok(())
}

fn validate_snapshot(snapshot: PathBuf, sample_size: usize) -> Result<()> {
    use kv_store::KvStore;
    use rand::seq::SliceRandom;
    use rand::Rng;

    info!(snapshot = %snapshot.display(), "Validating snapshot");

    let store = KvStore::open(&snapshot).context("Failed to open snapshot")?;
    let record_count = store.len();

    info!(record_count, "Snapshot opened successfully");

    // Collect all keys for sampling
    let keys: Vec<_> = store.iter().map(|(k, _)| *k).collect();

    // Sample random keys and verify lookups
    let mut rng = rand::thread_rng();
    let sample: Vec<_> = keys
        .choose_multiple(&mut rng, sample_size.min(keys.len()))
        .collect();

    info!(sample_size = sample.len(), "Verifying random sample");

    let pb = ProgressBar::new(sample.len() as u64);
    let mut errors = 0;

    for key in sample {
        if store.get(key).is_none() {
            warn!(key = ?&key[0..8], "Key lookup failed");
            errors += 1;
        }
        pb.inc(1);
    }

    pb.finish();

    if errors > 0 {
        anyhow::bail!("{} out of {} lookups failed", errors, sample_size);
    }

    info!("Validation passed: all {} sample lookups succeeded", sample_size);
    Ok(())
}

fn print_info(snapshot: PathBuf) -> Result<()> {
    use kv_store::KvStore;

    let store = KvStore::open(&snapshot).context("Failed to open snapshot")?;
    let info = store.version_info();

    println!("Snapshot Information");
    println!("====================");
    println!("File: {}", snapshot.display());
    println!("Record count: {}", info.record_count);
    println!("File size: {} bytes ({:.2} GB)", info.file_size, info.file_size as f64 / 1_073_741_824.0);
    println!(
        "Build timestamp: {} ({})",
        info.build_timestamp,
        chrono_format(info.build_timestamp)
    );
    println!("Data size: {} bytes ({:.2} GB)",
        info.record_count * RECORD_SIZE as u64,
        (info.record_count * RECORD_SIZE as u64) as f64 / 1_073_741_824.0
    );

    Ok(())
}

fn string_to_key(s: &str) -> [u8; KEY_SIZE] {
    let mut key = [0u8; KEY_SIZE];
    let bytes = s.as_bytes();
    let len = bytes.len().min(KEY_SIZE);
    key[..len].copy_from_slice(&bytes[..len]);
    key
}

fn string_to_value(s: &str) -> [u8; VALUE_SIZE] {
    let mut value = [0u8; VALUE_SIZE];
    let bytes = s.as_bytes();
    let len = bytes.len().min(VALUE_SIZE);
    value[..len].copy_from_slice(&bytes[..len]);
    value
}

fn chrono_format(timestamp: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let dt = UNIX_EPOCH + Duration::from_secs(timestamp);
    format!("{:?}", dt)
}
