//! Offline cloud contracts; all requests use an injected in-memory transport.

#[path = "github_cloud/cli.rs"]
mod cli;
#[path = "github_cloud/control.rs"]
mod control;
#[path = "github_cloud/control_support.rs"]
mod control_support;
#[path = "github_cloud/http.rs"]
mod http;
#[path = "github_cloud/ownership.rs"]
mod ownership;
#[path = "github_cloud/paging.rs"]
mod paging;
#[path = "github_cloud/recovery.rs"]
mod recovery;
#[path = "github_cloud/send_races.rs"]
mod send_races;
#[path = "github_cloud/support.rs"]
mod support;
#[path = "github_cloud/wire.rs"]
mod wire;
