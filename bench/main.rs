// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fs,
    process::{Command, Stdio},
};
use utils::root_path;

mod utils;

fn read_json(filename: &str) -> Result<Value> {
    let f = fs::File::open(filename)?;
    Ok(serde_json::from_reader(f)?)
}

fn write_json(filename: &str, value: &Value) -> Result<()> {
    let f = fs::File::create(filename)?;
    serde_json::to_writer(f, value)?;
    Ok(())
}

/// The list of the examples of the benchmark name, arguments and return code
const EXEC_TIME_BENCHMARKS: &[(&str, &str, Option<i32>)] = &[
    (
        "electron_hello_world",
        "apps/hello_world/out/startup-electron-linux-x64/startup-electron",
        None,
    ),
    (
        "electron_cpu_intensive",
        "apps/cpu_intensive/out/cpu-intensive-linux-x64/cpu-intensive",
        None,
    ),
    (
        "electron_3mb_transfer",
        "apps/file_transfer/out/file-transfer-linux-x64/file-transfer",
        None,
    ),
];

fn run_strace_benchmarks(new_data: &mut BenchResult) -> Result<()> {
    use std::io::Read;

    let mut thread_count = HashMap::<String, u64>::new();
    let mut syscall_count = HashMap::<String, u64>::new();

    for (name, example_exe, _) in EXEC_TIME_BENCHMARKS {
        let mut file = tempfile::NamedTempFile::new()?;

        Command::new("strace")
            .args(&[
                "-c",
                "-f",
                "-o",
                file.path().to_str().unwrap(),
                utils::root_path().join(example_exe).to_str().unwrap(),
            ])
            .stdout(Stdio::inherit())
            .spawn()?
            .wait()?;

        let mut output = String::new();
        file.as_file_mut().read_to_string(&mut output)?;

        let strace_result = utils::parse_strace_output(&output);
        let clone = strace_result.get("clone").map(|d| d.calls).unwrap_or(0) + 1;
        let total = strace_result.get("total").unwrap().calls;
        thread_count.insert(name.to_string(), clone);
        syscall_count.insert(name.to_string(), total);
    }

    new_data.thread_count = thread_count;
    new_data.syscall_count = syscall_count;

    Ok(())
}

fn run_max_mem_benchmark() -> Result<HashMap<String, u64>> {
    let mut results = HashMap::<String, u64>::new();

    for (name, example_exe, return_code) in EXEC_TIME_BENCHMARKS {
        let proc = Command::new("time")
            .args(&["-v", utils::root_path().join(example_exe).to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;

        let proc_result = proc.wait_with_output()?;
        if let Some(code) = return_code {
            assert_eq!(proc_result.status.code().unwrap(), *code);
        }
        let out = String::from_utf8(proc_result.stderr)?;

        results.insert(name.to_string(), utils::parse_max_mem(&out).unwrap());
    }

    Ok(results)
}

fn get_binary_sizes() -> Result<HashMap<String, u64>> {
    let mut sizes = HashMap::<String, u64>::new();
    // add size for all EXEC_TIME_BENCHMARKS
    for (name, example_exe, _) in EXEC_TIME_BENCHMARKS {
        let meta = std::fs::metadata(example_exe).unwrap();
        sizes.insert(name.to_string(), meta.len());
    }

    Ok(sizes)
}

const RESULT_KEYS: &[&str] = &["mean", "stddev", "user", "system", "min", "max"];

fn run_exec_time() -> Result<HashMap<String, HashMap<String, f64>>> {
    let benchmark_file = root_path().join("hyperfine_results.json");
    let benchmark_file = benchmark_file.to_str().unwrap();

    let mut command = [
        "hyperfine",
        "--export-json",
        benchmark_file,
        "--warmup",
        "3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();

    for (_, example_exe, _return_code) in EXEC_TIME_BENCHMARKS {
        command.push(
            utils::root_path()
                .join(example_exe)
                .to_str()
                .unwrap()
                .to_string(),
        );
    }

    utils::run(&command.iter().map(|s| s.as_ref()).collect::<Vec<_>>());

    let mut results = HashMap::<String, HashMap<String, f64>>::new();
    let hyperfine_results = read_json(benchmark_file)?;
    for ((name, _, _), data) in EXEC_TIME_BENCHMARKS.iter().zip(
        hyperfine_results
            .as_object()
            .unwrap()
            .get("results")
            .unwrap()
            .as_array()
            .unwrap(),
    ) {
        let data = data.as_object().unwrap().clone();
        results.insert(
            name.to_string(),
            data.into_iter()
                .filter(|(key, _)| RESULT_KEYS.contains(&key.as_str()))
                .map(|(key, val)| (key, val.as_f64().unwrap()))
                .collect(),
        );
    }

    Ok(results)
}

#[derive(Default, Serialize, Debug)]
struct BenchResult {
    created_at: String,
    sha1: String,
    exec_time: HashMap<String, HashMap<String, f64>>,
    binary_size: HashMap<String, u64>,
    max_memory: HashMap<String, u64>,
    thread_count: HashMap<String, u64>,
    syscall_count: HashMap<String, u64>,
}

fn main() -> Result<()> {
    if !env::args().any(|s| s == "--bench") {
        return Ok(());
    }

    println!("Starting electron benchmark");

    env::set_current_dir(&utils::root_path())?;

    println!("{:?}", &utils::root_path());

    let mut new_data = BenchResult {
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        sha1: utils::run_collect(&["git", "rev-parse", "HEAD"])
            .0
            .trim()
            .to_string(),
        exec_time: run_exec_time()?,
        binary_size: get_binary_sizes()?,
        ..Default::default()
    };

    if cfg!(target_os = "linux") {
        run_strace_benchmarks(&mut new_data)?;
        new_data.max_memory = run_max_mem_benchmark()?;
    }

    println!("===== <BENCHMARK RESULTS>");
    serde_json::to_writer_pretty(std::io::stdout(), &new_data)?;
    println!("\n===== </BENCHMARK RESULTS>");

    if let Some(filename) = root_path().join("bench.json").to_str() {
        write_json(filename, &serde_json::to_value(&new_data)?)?;
    } else {
        eprintln!("Cannot write bench.json, path is invalid");
    }

    Ok(())
}
