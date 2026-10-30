pub mod core;
mod history;
mod host;
pub mod ipc;
pub mod placement;
pub mod notifications;
pub mod protocol;
mod runtime;
pub mod templates;
mod tray;
mod visual;

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use protocol::{Mode, Request};

#[derive(Debug, Parser)]
#[command(name = "popu", version, about = "Show and manage local interaction requests")]
struct Cli { #[command(subcommand)] command: Commands }

#[derive(Debug, Subcommand)]
enum Commands {
    /// Submit a request; dialogs wait for their terminal response.
    Request(RunArgs),
    /// Replace a live request's complete content snapshot.
    Update { request_id: uuid::Uuid, update: PathBuf },
    /// Cancel a queued or shown request.
    Dismiss { request_id: uuid::Uuid },
    /// Inspect a request.
    Status { request_id: uuid::Uuid },
    /// Wait until a request reaches a terminal state.
    Wait { request_id: uuid::Uuid },
    /// Inspect or control the persistent Host.
    Host { #[command(subcommand)] command: HostCommands },
    /// Inspect and validate templates.
    Templates { #[command(subcommand)] command: TemplateCommands },
}

#[derive(Debug, Subcommand)]
enum HostCommands { Status, Stop, #[command(hide = true)] Run }

#[derive(Debug, Args, Clone)]
struct RunArgs {
    /// JSON request file, or - for stdin.
    #[arg(value_name = "REQUEST")]
    request: PathBuf,
    /// Trusted local template directory. This is not part of request JSON.
    #[arg(long)]
    template: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum TemplateCommands {
    List,
    Validate { #[arg(long)] path: PathBuf },
}

pub fn run() -> i32 {
    match run_inner() {
        Ok(code) => code,
        Err(error) => { eprintln!("Popu error: {error}"); runtime::EXIT_ERROR }
    }
}

fn run_inner() -> Result<i32, Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Commands::Request(args) => {
            let request = load_request(&args.request)?;
            let mode = request.mode;
            let template = args.template.map(|path| path.canonicalize()).transpose()?;
            let _ = templates::resolve(&request, template.as_deref())?;
            let created = rpc_with_start("request.create", serde_json::json!({"request":request,"template":template}))?;
            println!("{}", serde_json::to_string(&created)?);
            if mode == Mode::Notify { return Ok(runtime::EXIT_OK); }
            let id = created.get("request_id").and_then(|v| v.as_str()).ok_or("Host response has no request_id")?;
            let terminal = rpc_with_start("request.wait", serde_json::json!({"request_id":id}))?;
            if let Some(response) = terminal.get("response") { println!("{}", serde_json::to_string(response)?); }
            Ok(terminal.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(runtime::EXIT_ERROR as i64) as i32)
        }
        Commands::Update { request_id, update } => {
            let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(update)?)?;
            let mut params = value.as_object().cloned().ok_or("update must be an object")?;
            params.insert("request_id".into(), serde_json::json!(request_id));
            print_rpc(rpc_with_start("request.update", serde_json::Value::Object(params))?)
        }
        Commands::Dismiss { request_id } => print_rpc(rpc_with_start("request.dismiss", serde_json::json!({"request_id":request_id}))?),
        Commands::Status { request_id } => print_rpc(rpc_with_start("request.get", serde_json::json!({"request_id":request_id}))?),
        Commands::Wait { request_id } => print_rpc(rpc_with_start("request.wait", serde_json::json!({"request_id":request_id}))?),
        Commands::Host { command: HostCommands::Run } => Ok(host::run()),
        Commands::Host { command: HostCommands::Status } => print_rpc(rpc_call("host.status", serde_json::json!({}))?),
        Commands::Host { command: HostCommands::Stop } => print_rpc(rpc_call("host.stop", serde_json::json!({}))?),
        Commands::Templates { command: TemplateCommands::List } => {
            println!("{}", serde_json::to_string(templates::builtin_names())?); Ok(runtime::EXIT_OK)
        }
        Commands::Templates { command: TemplateCommands::Validate { path } } => {
            let name = templates::validate_directory(&path)?;
            println!("{}", serde_json::to_string(&serde_json::json!({"status":"valid","template":name}))?);
            Ok(runtime::EXIT_OK)
        }
    }
}

fn print_rpc(value: serde_json::Value) -> Result<i32, Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string(&value)?); Ok(runtime::EXIT_OK)
}

fn rpc_call(method: &str, params: serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let response = ipc::call(&rpc_request(method, params))?;
    unwrap_rpc(response)
}

fn rpc_request(method: &str, params: serde_json::Value) -> ipc::RpcRequest {
    ipc::RpcRequest { version: ipc::RPC_VERSION.into(), method: method.into(), params }
}

fn unwrap_rpc(response: ipc::RpcResponse) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if response.ok { Ok(response.result.unwrap_or(serde_json::Value::Null)) }
    else { Err(format!("Host RPC failed: {}", response.error.unwrap_or(serde_json::Value::Null)).into()) }
}

fn rpc_with_start(method: &str, params: serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match ipc::call(&rpc_request(method, params.clone())) {
        Ok(response) => return unwrap_rpc(response),
        Err(_) => {}
    }
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["host", "run"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
    let _child = command.spawn()?;
    let mut last = None;
    for _ in 0..60 {
        thread::sleep(Duration::from_millis(50));
        match ipc::call(&rpc_request(method, params.clone())) {
            Ok(response) => return unwrap_rpc(response),
            Err(error) => last = Some(error.into()),
        }
    }
    Err(last.unwrap_or_else(|| "Host did not become ready".into()))
}

fn load_request(path: &Path) -> Result<Request, Box<dyn std::error::Error>> {
    let text = if path == Path::new("-") {
        let mut text = String::new(); io::stdin().read_to_string(&mut text)?; text
    } else { fs::read_to_string(path)? };
    Ok(Request::parse(&text)?)
}
