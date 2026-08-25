//! Browser dashboard launcher over the canonical Bureau canvas server.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::{Context as _, bail};
use clap::Args;

const SOURCE_SERVER: &str = ".github/extensions/bureau-canvas/serve.mjs";

/// Browser dashboard options.
#[derive(Debug, Args)]
pub struct DashboardArgs {
    /// Config directory to display.
    #[arg(long, default_value = ".bureau")]
    dir: PathBuf,
    /// Pipeline to open initially.
    #[arg(long)]
    pipeline: Option<String>,
    /// Reload browser pages when dashboard web files change.
    #[arg(long)]
    dev: bool,
    /// Print the URL without opening a browser.
    #[arg(long)]
    no_open: bool,
    /// Exact loopback port; by default the operating system chooses one.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    port: Option<u16>,
    /// Dashboard server override for integration testing.
    #[arg(long, hide = true)]
    server: Option<PathBuf>,
    /// Node executable override for integration testing.
    #[arg(long, hide = true)]
    node: Option<PathBuf>,
}

fn temporary_path(path: &Path) -> anyhow::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("dashboard asset path has no file name"))?
        .to_string_lossy();
    Ok(path.with_file_name(format!(".{name}.{}.tmp", std::process::id())))
}

fn write_asset(root: &Path, relative: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("dashboard asset has no parent: {relative}"))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temporary = temporary_path(&path)?;
    fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, &path).with_context(|| format!("installing {}", path.display()))
}

fn materialize_server() -> anyhow::Result<PathBuf> {
    let home = bureau::home::Home::discover()?;
    let directory = home
        .layout()
        .root()
        .join("dashboard")
        .join(bureau_dashboard::BUNDLE_ID);
    for (relative, bytes) in bureau_dashboard::FILES {
        write_asset(&directory, relative, bytes)?;
    }
    Ok(directory.join("serve.mjs"))
}

fn existing_server(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.is_file() {
        return Ok(path);
    }
    bail!("Bureau dashboard server does not exist: {}", path.display())
}

fn development_server() -> anyhow::Result<PathBuf> {
    existing_server(PathBuf::from(SOURCE_SERVER))
        .context("development mode must run from a Bureau source checkout")
}

fn server_path(args: &DashboardArgs) -> anyhow::Result<PathBuf> {
    if let Some(server) = &args.server {
        return existing_server(server.clone());
    }
    if args.dev {
        return development_server();
    }
    materialize_server()
}

fn configure_runtime(command: &mut Command, args: &DashboardArgs) {
    command.args(args.dev.then_some("--dev"));
    if let Some(port) = args.port {
        command.arg("--port").arg(port.to_string());
    }
    command.args((!args.no_open).then_some("--open"));
}

fn configure(command: &mut Command, server: &Path, bureau: &Path, args: &DashboardArgs) {
    command
        .arg(server)
        .arg("--bureau")
        .arg(bureau)
        .arg("--dir")
        .arg(&args.dir);
    if let Some(pipeline) = &args.pipeline {
        command.arg("--pipeline").arg(pipeline);
    }
    configure_runtime(command, args);
}

fn launch(
    node: &OsStr,
    server: &Path,
    bureau: &Path,
    args: &DashboardArgs,
) -> io::Result<ExitStatus> {
    let mut command = Command::new(node);
    configure(&mut command, server, bureau, args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        Err(command.exec())
    }
    #[cfg(not(unix))]
    {
        command.status()
    }
}

fn status(server: &Path, bureau: &Path, args: &DashboardArgs) -> io::Result<ExitStatus> {
    if let Some(node) = &args.node {
        return launch(node.as_os_str(), server, bureau, args);
    }
    match launch(OsStr::new("node"), server, bureau, args) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            launch(OsStr::new("node.exe"), server, bureau, args)
        }
        result => result,
    }
}

/// Runs the dashboard server in the foreground.
pub fn run(args: &DashboardArgs) -> anyhow::Result<i32> {
    let server = server_path(args)?;
    let bureau = std::env::current_exe().context("locating the Bureau executable")?;
    let status = status(&server, &bureau, args).context("starting the Bureau dashboard server")?;
    Ok(status.code().unwrap_or(1))
}
