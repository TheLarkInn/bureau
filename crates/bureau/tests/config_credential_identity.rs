//! The reserved config credential is proved before the reviewed config
//! is fetched with it (DESIGN.md section 7).
//!
//! Offline: a loopback server on 127.0.0.1 stands in for the forge and
//! the config remote points at it, so a run that fetched first would
//! fail as Git rather than as an identity.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The resolved value; no failure may echo it.
const TOKEN: &str = "config-token-value-must-not-appear";
/// A forge that accepts the value as somebody else entirely.
const OTHER_ACCOUNT: &str = r#"{"login":"someone-else"}"#;
/// A port nothing listens on, so a fetch fails at once.
const CLOSED_REMOTE: &str = "http://127.0.0.1:1/acme/config";

static NEXT: AtomicU32 = AtomicU32::new(0);

/// One canned identity answer on 127.0.0.1, counting what it was asked.
struct Forge {
    addr: SocketAddr,
    join: JoinHandle<usize>,
}

impl Forge {
    fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().expect("local addr");
        let join = std::thread::spawn(move || serve_once(&listener, body));
        Self { addr, join }
    }

    fn remote(&self) -> String {
        format!("http://{}/acme/config", self.addr)
    }

    /// How many calls it answered; joins the server thread first.
    fn served(self) -> usize {
        self.join.join().expect("server thread")
    }
}

fn serve_once(listener: &TcpListener, body: &str) -> usize {
    let mut stream = accept(listener);
    read_request(&stream);
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("write");
    1
}

/// The next connection; the deadline turns a check that never happened
/// into a failed test instead of a hung one.
fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok((stream, _)) = listener.accept() {
            return stream;
        }
        assert!(Instant::now() < deadline, "no identity call arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn read_request(stream: &TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read line");
        if line == "\r\n" || line.is_empty() {
            return;
        }
    }
}

fn settings_yaml(remote: &str, identity: Option<&str>) -> String {
    let declared = identity.map_or_else(String::new, |value| format!("    identity: {value}\n"));
    let source = "  config:\n    source: environment\n    variable: BUREAU_CONFIG_TOKEN\n";
    format!(
        "config:\n  kind: separate_repository\n  remote: '{remote}'\n  reference: main\ncredentials:\n{source}{declared}"
    )
}

/// The fixed first pipeline, so `init` authors nothing and reaches the
/// config pull request on local work alone.
const INIT_TAIL: &str = concat!(
    "repositories:\n  code:\n    url: https://github.com/example/code\n",
    "    forge: github\n    access: pr\n    credential: config\n",
    "assignment:\n  name: first\n  work:\n    forge: github\n",
    "    source: example/code\n    filter: is:issue\n",
    "    abort_label: bureau:failed\n    escalate_label: bureau:needs-human\n",
    "  primary_repo: code\n  verify: \"true\"\n  branch_prefix: bureau/\n",
    "  adapter: fake\nfirst_pipeline:\n  kind: fixed\n",
);

fn init_request(remote: &str, identity: &str) -> String {
    use std::fmt::Write as _;

    let settings =
        settings_yaml(remote, Some(identity))
            .lines()
            .fold(String::new(), |mut block, line| {
                let _written = writeln!(block, "  {line}");
                block
            });
    format!("settings:\n{settings}{INIT_TAIL}")
}

/// A bureau home whose only content is settings naming a config remote.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn empty(label: &str) -> Self {
        let root = PathBuf::from("target/config-identity-tests").join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _removed = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("home");
        Self { root }
    }

    fn new(label: &str, remote: &str, identity: Option<&str>) -> Self {
        let fixture = Self::empty(label);
        std::fs::write(
            fixture.root.join("settings.yaml"),
            settings_yaml(remote, identity),
        )
        .expect("settings");
        fixture
    }

    fn run(&self) -> Output {
        self.bureau(&["run", "fix-failing-test", "--item", "42"])
    }

    fn reconcile(&self) -> Output {
        self.bureau(&["reconcile", "--now"])
    }

    /// `init` against a request naming this remote and identity; the
    /// home stays without settings, which is what `init` requires.
    fn init(&self, remote: &str, identity: &str) -> Output {
        let path = self.root.join("init.yaml");
        std::fs::write(&path, init_request(remote, identity)).expect("request");
        self.bureau(&["init", "--from", &path.to_string_lossy()])
    }

    fn bureau(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bureau"))
            .args(args)
            .env("BUREAU_HOME", &self.root)
            .env("BUREAU_CONFIG_TOKEN", TOKEN)
            .output()
            .expect("bureau")
    }

    /// Whether the config fetch got as far as creating its snapshots.
    fn fetched(&self) -> bool {
        self.root.join("config-cache").join("snapshots").exists()
    }

    /// Whether `init` got as far as mirroring the config remote.
    fn mirrored(&self) -> bool {
        self.root.join("config-cache").join("init-mirrors").exists()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.root);
    }
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A config credential that authenticates as another account fails the
/// verb before the config repo is cloned: the fetch never starts, and
/// neither the value nor a response body reaches the message.
#[test]
fn a_wrong_config_identity_fails_before_the_config_is_fetched() {
    let forge = Forge::start(OTHER_ACCOUNT);
    let fixture = Fixture::new("wrong-identity", &forge.remote(), Some("bureau-bot"));
    let output = fixture.run();
    let stderr = stderr_of(&output);
    let named = stderr.contains("someone-else") && stderr.contains("bureau-bot");
    assert_eq!(
        (
            output.status.code(),
            named,
            stderr.contains(TOKEN),
            fixture.fetched(),
            forge.served(),
        ),
        (Some(2), true, false, false, 1)
    );
}

/// Declaring no identity leaves the fetch its own check: no forge is
/// asked anything, and the run fails where it always did — loading the
/// committed config.
#[test]
fn a_config_credential_without_a_declared_identity_reaches_the_fetch() {
    let fixture = Fixture::new("no-identity", CLOSED_REMOTE, None);
    let output = fixture.run();
    let stderr = stderr_of(&output);
    assert_eq!(
        (
            output.status.code(),
            stderr.contains("loading committed config"),
            stderr.contains("could not be verified"),
            stderr.contains(TOKEN),
        ),
        (Some(2), true, false, false)
    );
}

/// The daemon proves its configured config credential once at startup:
/// a declaration the forge cannot settle stops the first pass before
/// the config is cloned, so no pass can spawn a run from a fetch that
/// used an unproven value.
#[test]
fn the_daemon_verifies_the_config_credential_before_its_first_fetch() {
    let fixture = Fixture::new("daemon-gate", CLOSED_REMOTE, Some("bureau-bot"));
    let output = fixture.reconcile();
    let stderr = stderr_of(&output);
    assert_eq!(
        (
            output.status.code(),
            stderr.contains("could not be verified"),
            stderr.contains("git clone"),
            fixture.fetched(),
        ),
        (Some(2), true, false, false)
    );
}

/// And a daemon whose config credential declares nothing asks no forge
/// anything: it goes straight to the fetch, pass after pass.
#[test]
fn a_daemon_without_a_declared_identity_reaches_the_fetch() {
    let fixture = Fixture::new("daemon-undeclared", CLOSED_REMOTE, None);
    let output = fixture.reconcile();
    let stderr = stderr_of(&output);
    assert_eq!(
        (
            output.status.code(),
            stderr.contains("git clone"),
            stderr.contains("could not be verified"),
            stderr.contains(TOKEN),
        ),
        (Some(2), true, false, false)
    );
}

/// `init` opens the config pull request as this credential, so it is
/// proved first: a wrong account stops the flow before the config
/// remote is mirrored, cloned, or pushed to.
#[test]
fn init_verifies_the_config_credential_before_it_touches_the_remote() {
    let forge = Forge::start(OTHER_ACCOUNT);
    let fixture = Fixture::empty("init-gate");
    let output = fixture.init(&forge.remote(), "bureau-bot");
    let stderr = stderr_of(&output);
    let named = stderr.contains("someone-else") && stderr.contains("bureau-bot");
    assert_eq!(
        (
            output.status.code(),
            named,
            stderr.contains(TOKEN),
            fixture.mirrored(),
            forge.served(),
        ),
        (Some(2), true, false, false, 1)
    );
}
