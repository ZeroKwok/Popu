mod lifetime;
pub mod placement;
pub mod protocol;
mod runtime;
pub mod templates;

use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use clap::{Args, Parser, Subcommand};

use protocol::{Mode, Request};

#[derive(Debug, Parser)]
#[command(
    name = "popu",
    version,
    about = "Show a local interaction dialog or notification"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Show a dialog and write its response JSON to stdout.
    Dialog(RunArgs),
    /// Show a fire-and-forget notification.
    Notify(RunArgs),
    /// Inspect and validate templates.
    Templates {
        #[command(subcommand)]
        command: TemplateCommands,
    },
    #[command(name = "__notify-child", hide = true)]
    NotifyChild(RunArgs),
}

#[derive(Debug, Args, Clone)]
struct RunArgs {
    /// JSON request file. Use '-' to read a dialog request from stdin.
    #[arg(long)]
    request: PathBuf,
    /// Trusted local template directory. This is not part of request JSON.
    #[arg(long)]
    template_dir: Option<PathBuf>,
    /// Inherited lifetime pipe read handle. Intended for SDK use.
    #[arg(long, hide = true)]
    lifetime_handle: Option<u64>,
    /// Retained for a readable CLI contract; dialogs always wait.
    #[arg(long, default_value_t = false)]
    wait: bool,
}

#[derive(Debug, Subcommand)]
enum TemplateCommands {
    /// List built-in templates.
    List,
    /// Validate a trusted local template directory.
    Validate {
        #[arg(long)]
        path: PathBuf,
    },
}

pub fn run() -> i32 {
    match run_inner() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Popu error: {error}");
            runtime::EXIT_ERROR
        }
    }
}

fn run_inner() -> Result<i32, Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Dialog(args) => {
            let request = load_request(&args.request)?;
            require_mode(&request, Mode::Dialog)?;
            Ok(runtime::run_ui(
                request,
                args.template_dir.as_deref(),
                args.lifetime_handle,
                false,
            ))
        }
        Commands::Notify(args) => run_notification_parent(args),
        Commands::NotifyChild(args) => {
            let request = load_request(&args.request)?;
            require_mode(&request, Mode::Notify)?;
            Ok(runtime::run_ui(
                request,
                args.template_dir.as_deref(),
                args.lifetime_handle,
                true,
            ))
        }
        Commands::Templates { command } => match command {
            TemplateCommands::List => {
                println!("{}", serde_json::to_string(templates::builtin_names())?);
                Ok(runtime::EXIT_OK)
            }
            TemplateCommands::Validate { path } => {
                let name = templates::validate_directory(&path)?;
                println!(
                    "{{\"status\":\"valid\",\"template\":{}}}",
                    serde_json::to_string(&name)?
                );
                Ok(runtime::EXIT_OK)
            }
        },
    }
}

fn run_notification_parent(args: RunArgs) -> Result<i32, Box<dyn std::error::Error>> {
    if args.request == Path::new("-") {
        return Err(
            "notify requires a request file because its UI outlives the CLI process".into(),
        );
    }
    let request = load_request(&args.request)?;
    require_mode(&request, Mode::Notify)?;
    let _ = templates::resolve(&request, args.template_dir.as_deref())?;

    let executable = std::env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg("__notify-child")
        .arg("--request")
        .arg(&args.request)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // Keep the short-lived UI child from opening a second console when
        // Popu is called by a GUI program or automation runner.
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    if let Some(template_dir) = args.template_dir {
        command.arg("--template-dir").arg(template_dir);
    }
    if let Some(handle) = args.lifetime_handle {
        command.arg("--lifetime-handle").arg(handle.to_string());
    }
    let (mut child, detached_stdout) = spawn_notification_child(&mut command)?;
    let mut stdout = io::BufReader::new(
        child
            .stdout
            .take()
            .ok_or("notification child has no stdout")?,
    );
    let mut first = String::new();
    stdout.read_line(&mut first)?;
    let first = first.trim_end();
    if first.is_empty() {
        let status = child.wait()?;
        let mut diagnostics = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            stderr.read_to_string(&mut diagnostics)?;
        }
        let diagnostics = diagnostics.trim();
        if diagnostics.is_empty() {
            return Err(format!("notification failed before it was shown: {status}").into());
        }
        return Err(format!("notification failed before it was shown: {diagnostics}").into());
    }
    let ready: serde_json::Value = serde_json::from_str(first)?;
    if ready.get("status").and_then(|value| value.as_str()) != Some("shown") {
        return Err(format!("unexpected notification readiness response: {first}").into());
    }
    write_notification_ready(first, detached_stdout)?;
    Ok(runtime::EXIT_OK)
}

#[cfg(windows)]
fn spawn_notification_child(command: &mut Command) -> io::Result<(Child, Option<fs::File>)> {
    use std::os::windows::io::FromRawHandle;

    use windows_sys::Win32::Foundation::{
        DuplicateHandle, GetHandleInformation, SetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE,
        HANDLE_FLAG_INHERIT,
    };
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut output_copy: HANDLE = std::ptr::null_mut();
    let mut restore = Vec::<(HANDLE, u32)>::new();
    unsafe {
        let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
        let process = GetCurrentProcess();
        if !stdout.is_null()
            && DuplicateHandle(
                process,
                stdout,
                process,
                &mut output_copy,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            ) == 0
        {
            output_copy = std::ptr::null_mut();
        }

        // Capturing callers expose inheritable stdout/stderr pipe handles.
        // The UI child needs only the explicit pipes configured on `Command`.
        for kind in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(kind);
            let mut flags = 0;
            if !handle.is_null()
                && GetHandleInformation(handle, &mut flags) != 0
                && flags & HANDLE_FLAG_INHERIT != 0
                && SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) != 0
            {
                restore.push((handle, flags));
            }
        }
    }

    let result = command.spawn();
    unsafe {
        for (handle, flags) in restore {
            let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags);
        }
    }
    let output = if output_copy.is_null() {
        None
    } else {
        Some(unsafe { fs::File::from_raw_handle(output_copy) })
    };
    result.map(|child| (child, output))
}

#[cfg(not(windows))]
fn spawn_notification_child(command: &mut Command) -> io::Result<(Child, Option<fs::File>)> {
    command.spawn().map(|child| (child, None))
}

fn write_notification_ready(first: &str, detached_stdout: Option<fs::File>) -> io::Result<()> {
    if let Some(mut stdout) = detached_stdout {
        writeln!(stdout, "{first}")?;
        stdout.flush()
    } else {
        println!("{first}");
        io::stdout().flush()
    }
}

fn load_request(path: &Path) -> Result<Request, Box<dyn std::error::Error>> {
    let text = if path == Path::new("-") {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text)?;
        text
    } else {
        fs::read_to_string(path)?
    };
    Ok(Request::parse(&text)?)
}

fn require_mode(request: &Request, mode: Mode) -> Result<(), Box<dyn std::error::Error>> {
    if request.mode != mode {
        return Err(format!(
            "request mode {:?} does not match command {:?}",
            request.mode, mode
        )
        .into());
    }
    Ok(())
}
