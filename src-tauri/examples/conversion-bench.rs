// Benchmark helper: prepare one immutable Bedrock input for repeated b2j runs,
// and validate each Java output with the application's own validator.
// cargo run --release --example conversion-bench -- prepare <archive.zip> <new-work-dir>
// cargo run --release --example conversion-bench -- validate <java-world-dir>

use nwc_lib::{archive, decrypt, detect, log::AppLog, sink::Sink, validate};
use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

fn ensure_new_directory(path: &Path) -> io::Result<()> {
    if path.exists() {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("benchmark directory already exists: {}", path.display()),
        ))
    } else {
        Ok(())
    }
}

fn sink(log_path: &Path) -> io::Result<Sink> {
    let log = Arc::new(AppLog::new(log_path)?);
    Ok(Sink::new(
        "benchmark".into(),
        Arc::new(AtomicBool::new(false)),
        log,
        |_| {},
    ))
}

fn prepare(archive_path: &Path, work: &Path) -> Result<(), Box<dyn std::error::Error>> {
    ensure_new_directory(work)?;
    if let Some(parent) = work.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(work)?;
    let sink = sink(&work.join("benchmark.log"))?;
    let extracted = work.join("extracted");
    let start = Instant::now();
    archive::extract_zip(archive_path, &extracted, &sink)?;
    let extract_seconds = start.elapsed().as_secs_f64();
    let world = detect::detect(&extracted, &sink.log)?;
    if !world.world_type.is_bedrock() {
        return Err("input is not a Bedrock world".into());
    }
    let bedrock = work.join("bedrock");
    let start = Instant::now();
    decrypt::prepare(&world, &bedrock, &sink)?;
    let decrypt_seconds = start.elapsed().as_secs_f64();
    println!(
        "{}",
        serde_json::json!({
            "bedrock": bedrock,
            "extract_seconds": extract_seconds,
            "decrypt_seconds": decrypt_seconds,
            "world_files": world.file_count,
            "world_bytes": world.byte_count,
        })
    );
    Ok(())
}

fn check(world: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let sink = sink(&temp.path().join("benchmark.log"))?;
    let start = Instant::now();
    let result = validate::validate(world, &sink)?;
    println!(
        "{}",
        serde_json::json!({
            "validate_seconds": start.elapsed().as_secs_f64(),
            "validation": result,
        })
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().collect();
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("prepare") if args.len() == 4 => {
            prepare(Path::new(&args[2]), Path::new(&args[3]))
        }
        Some("validate") if args.len() == 3 => check(Path::new(&args[2])),
        _ => Err("usage: conversion-bench prepare <archive.zip> <new-work-dir> | validate <java-world-dir>".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_to_reuse_a_benchmark_directory() {
        let root = tempfile::tempdir().unwrap();
        assert!(super::ensure_new_directory(&root.path().join("new")).is_ok());
        assert!(super::ensure_new_directory(root.path()).is_err());
    }
}
