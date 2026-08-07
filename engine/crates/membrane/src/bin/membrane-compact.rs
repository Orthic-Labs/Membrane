use membrane::compression::{compact_and_store, retrieve_raw, CompactorKind};
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(error) => {
            eprintln!("membrane-compact: {error}");
            ExitCode::from(2)
        }
    }
}
fn run(args: Vec<String>) -> Result<i32, String> {
    if args.len() > 256 || args.iter().any(|arg| arg.len() > 8192) {
        return Err("argument limits exceeded".into());
    }
    match args.first().map(String::as_str) { Some("get") => get(&args[1..]), Some("run") => execute(&args[1..]), _ => Err("usage: membrane-compact get DIGEST --raw-root DIR | run KIND --raw-root DIR [--max-lines N] -- COMMAND [ARGS...]".into()) }
}
fn get(args: &[String]) -> Result<i32, String> {
    match args {
        [digest, flag, root] if flag == "--raw-root" => {
            print!("{}", retrieve_raw(PathBuf::from(root).as_path(), digest)?)
        }
        _ => return Err("usage: get DIGEST --raw-root DIR".into()),
    }
    Ok(0)
}
#[rustfmt::skip]
fn execute(args: &[String]) -> Result<i32, String> {
    let kind = CompactorKind::parse(args.first().ok_or("run requires KIND")?).ok_or("kind must be git, tests, build, package, containers, or logs")?;
    let mut index = 1;
    if args.get(index).map(String::as_str) != Some("--raw-root") { return Err("run requires KIND --raw-root DIR before --".into()); }
    let root = PathBuf::from(args.get(index + 1).ok_or("--raw-root requires DIR")?); index += 2;
    let max_lines = if args.get(index).map(String::as_str) == Some("--max-lines") { index += 1; let value = args.get(index).ok_or("--max-lines requires N")?.parse::<usize>().map_err(|_| "--max-lines must be an integer")?; index += 1; value } else { 80 };
    if !(1..=10_000).contains(&max_lines) { return Err("--max-lines must be between 1 and 10000".into()); }
    if args.get(index).map(String::as_str) != Some("--") { return Err("run options must precede -- COMMAND; duplicate or unknown options are rejected".into()); }
    let command = args.get(index + 1).ok_or("run requires COMMAND after --")?;
    let mut child = Command::new(command).args(&args[index + 2..]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("execute {command}: {error}"))?;
    let total = Arc::new(AtomicUsize::new(0)); let exceeded = Arc::new(AtomicBool::new(false));
    let out = drain(child.stdout.take().unwrap(), total.clone(), exceeded.clone()); let err = drain(child.stderr.take().unwrap(), total, exceeded.clone());
    let status = loop { if exceeded.load(Ordering::Relaxed) { let _ = child.kill(); } if let Some(status) = child.try_wait().map_err(|e| format!("wait for {command}: {e}"))? { break status; } std::thread::sleep(Duration::from_millis(2)); };
    let mut bytes = out.join().map_err(|_| "stdout drain failed")?; bytes.extend(err.join().map_err(|_| "stderr drain failed")?);
    if exceeded.load(Ordering::Relaxed) { return Err(format!("command_output_limit_exceeded:{MAX_OUTPUT_BYTES}")); }
    let raw = String::from_utf8(bytes).map_err(|_| "command output is not UTF-8; refusing lossy compaction")?;
    let compacted = compact_and_store(kind, &raw, status.success(), &root, max_lines)?;
    println!("{}", serde_json::to_string(&compacted).map_err(|error| format!("serialize result: {error}"))?); Ok(status.code().unwrap_or(1))
}
#[rustfmt::skip]
fn drain(mut pipe: impl Read + Send + 'static, total: Arc<AtomicUsize>, exceeded: Arc<AtomicBool>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || { let mut kept = Vec::new(); let mut chunk = [0; 8192]; while let Ok(n) = pipe.read(&mut chunk) { if n == 0 { break; } if total.fetch_add(n, Ordering::Relaxed) + n <= MAX_OUTPUT_BYTES { kept.extend_from_slice(&chunk[..n]); } else { exceeded.store(true, Ordering::Relaxed); } } kept })
}
