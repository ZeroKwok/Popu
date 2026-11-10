pub mod config;
pub mod core;
mod history;
mod host;
mod host_client;
mod host_process;
mod preflight;
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
    /// Check Host configuration and initialize the default file if missing.
    Config { #[command(subcommand)] command: Option<ConfigCommands> },
    /// Inspect and validate templates.
    Templates { #[command(subcommand)] command: TemplateCommands },
}

#[derive(Debug, Subcommand)]
enum HostCommands { Status, Stop, #[command(hide = true)] Run }

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    /// Validate a TOML file without writing to disk.
    Validate {
        /// File to validate. Defaults to the Host's configuration path.
        path: Option<PathBuf>,
        #[arg(long = "path", hide = true, conflicts_with = "path")]
        legacy_path: Option<PathBuf>,
    },
    /// Create a starter file at the default configuration path.
    Init {
        /// Replace an existing configuration file with the starter template.
        #[arg(long)]
        force: bool,
    },
    /// Print the default configuration path.
    Path,
}

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
            let (request, payload) = load_request(&args.request)?;
            let mode = request.mode;
            let template = args.template.map(|path| path.canonicalize()).transpose()?;
            let _ = templates::resolve(&request, template.as_deref())?;
            let created = rpc_with_start("request.create", serde_json::json!({"request":payload,"template":template}))?;
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
        Commands::Host { command: HostCommands::Run } => { preflight::check()?; Ok(host::run()) },
        Commands::Host { command: HostCommands::Status } => print_rpc(rpc_call("host.status", serde_json::json!({}))?),
        Commands::Host { command: HostCommands::Stop } => print_rpc(rpc_call("host.stop", serde_json::json!({}))?),
        Commands::Config { command } => run_config(command),
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

fn run_config(command: Option<ConfigCommands>) -> Result<i32, Box<dyn std::error::Error>> {
    match command {
        None => {
            let path = config::default_path()?;
            let initialized = config::initialize_if_missing(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            config::validate_file(&path)?;
            print_rpc(serde_json::json!({"status":"valid","path":path,"initialized":initialized}))
        }
        Some(ConfigCommands::Validate { path, legacy_path }) => {
            let path = path.or(legacy_path).map_or_else(config::default_path, Ok)?;
            config::validate_file(&path)?;
            print_rpc(serde_json::json!({"status":"valid","path":path}))
        }
        Some(ConfigCommands::Init { force }) => {
            let path = config::default_path()?;
            if let Err(error) = config::initialize_file(&path, force) {
                let hint = if !force && error.kind() == io::ErrorKind::AlreadyExists {
                    " (use popu config init --force to replace it)"
                } else {
                    ""
                };
                return Err(format!("{}: {error}{hint}", path.display()).into());
            }
            print_rpc(serde_json::json!({"status":"initialized","path":path}))
        }
        Some(ConfigCommands::Path) => {
            println!("{}", config::default_path()?.display());
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
    unwrap_rpc(host_client::call(&rpc_request(method, params))?)
}

fn load_request(path: &Path) -> Result<(Request, serde_json::Value), Box<dyn std::error::Error>> {
    let text = if path == Path::new("-") {
        let mut text = String::new(); io::stdin().read_to_string(&mut text)?; text
    } else { fs::read_to_string(path)? };
    // Forward the original fields so omitted values can receive Host defaults.
    Ok((Request::parse(&text)?, serde_json::from_str(&text)?))
}

#[cfg(test)]
mod request_loading_tests {
    use super::*;

    #[test]
    fn forwards_omitted_and_explicit_timeouts_distinctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request.json");
        let mut payload = serde_json::json!({
            "version":"1.0", "mode":"notify", "content":{"title":"Done"},
        });
        fs::write(&path, payload.to_string()).unwrap();
        assert!(load_request(&path).unwrap().1.get("duration_ms").is_none());

        payload["duration_ms"] = serde_json::json!(8_000);
        fs::write(&path, payload.to_string()).unwrap();
        assert_eq!(load_request(&path).unwrap().1["duration_ms"], 8_000);
    }
}
