//! Typed CLI requests. Parsing rejects options that do not belong to a command.
use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "journal-rs",
    version,
    about = "Read and write the local Apple Journal store"
)]
pub struct Cli {
    #[arg(long, global = true, env = "JOURNAL_DB")]
    pub db: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}
impl Cli {
    pub fn database_path(&self) -> Result<PathBuf> {
        Ok(std::path::absolute(
            self.db
                .as_ref()
                .map(|p| expand(&p.to_string_lossy()))
                .unwrap_or_else(crate::store::default_db),
        )?)
    }
}
#[derive(Debug, Subcommand)]
pub enum Command {
    List(ListRequest),
    Show(ShowRequest),
    Search(SearchRequest),
    Export(ExportRequest),
    Stats(JsonOptions),
    Journals(JsonOptions),
    Deleted(JsonOptions),
    Doctor,
    SyncJournals(AuditRequest),
    Sandbox(SandboxRequest),
    Render(RenderRequest),
    Write(CreateRequest),
    Edit(EditRequest),
    Delete(DeleteRequest),
    Restore(RestoreRequest),
    Empty(EmptyRequest),
    RepairLocations(RepairRequest),
}
#[derive(Debug, Args, Default)]
pub struct JsonOptions {
    #[arg(long)]
    pub json: bool,
}
#[derive(Debug, Args, Default)]
pub struct ListOptions {
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub json: bool,
}
#[derive(Debug, Args)]
pub struct ListRequest {
    #[command(flatten)]
    pub output: ListOptions,
    #[arg(long, value_parser=crate::store::parse_date)]
    pub since: Option<f64>,
    #[arg(long, value_parser=crate::store::parse_date)]
    pub until: Option<f64>,
    #[arg(long)]
    pub include_empty: bool,
}
#[derive(Debug, Args)]
pub struct ShowRequest {
    pub id: i64,
    #[arg(long)]
    pub json: bool,
}
#[derive(Debug, Args)]
pub struct SearchRequest {
    #[arg(allow_hyphen_values = true)]
    pub query: String,
    #[command(flatten)]
    pub output: ListOptions,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Md,
    Json,
}
#[derive(Debug, Args)]
pub struct ExportRequest {
    #[arg(long, value_parser=parse_path)]
    pub dir: PathBuf,
    #[arg(long, value_enum, default_value = "md")]
    pub format: ExportFormat,
}
#[derive(Debug, Args)]
pub struct AuditRequest {
    #[arg(long)]
    pub journal: Option<String>,
    // Accepted upstream no-ops. This command remains read-only.
    #[arg(long)]
    pub live: bool,
    #[arg(long)]
    pub dry_run: bool,
}
#[derive(Debug, Args)]
pub struct SandboxRequest {
    #[arg(long, value_parser=parse_path)]
    pub dir: PathBuf,
    #[arg(long, value_parser=parse_path)]
    pub from: Option<PathBuf>,
}
#[derive(Debug, Args, Default)]
pub struct TextSource {
    #[arg(long, conflicts_with_all=["body_file","body_rtf"])]
    pub body: Option<String>,
    #[arg(long, value_parser=parse_path, conflicts_with="body_rtf")]
    pub body_file: Option<PathBuf>,
    #[arg(long, value_parser=parse_path, conflicts_with="markdown")]
    pub body_rtf: Option<PathBuf>,
    #[arg(long)]
    pub markdown: bool,
}
#[derive(Debug, Args)]
pub struct RenderRequest {
    #[arg(long, allow_hyphen_values = true, conflicts_with = "body_file")]
    pub body: Option<String>,
    #[arg(long, value_parser=parse_path)]
    pub body_file: Option<PathBuf>,
    #[arg(long, conflicts_with = "inline")]
    pub plain: bool,
    #[arg(long)]
    pub inline: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MapSize {
    #[default]
    Small,
    Large,
}
impl MapSize {
    pub fn slim(self) -> i64 {
        i64::from(self == Self::Small)
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Large => "large",
        }
    }
}
fn map_size(s: &str) -> std::result::Result<MapSize, String> {
    match s {
        "small" => Ok(MapSize::Small),
        "large" => Ok(MapSize::Large),
        "off" => Err("off is not supported; use small or large".into()),
        _ => Err("must be small or large".into()),
    }
}
fn latitude(s: &str) -> std::result::Result<f64, String> {
    coordinate(s, -90., 90.)
}
fn longitude(s: &str) -> std::result::Result<f64, String> {
    coordinate(s, -180., 180.)
}
fn coordinate(s: &str, min: f64, max: f64) -> std::result::Result<f64, String> {
    let n = s.parse::<f64>().map_err(|_| "invalid coordinate")?;
    if n.is_finite() && (min..=max).contains(&n) {
        Ok(n)
    } else {
        Err(format!("coordinate must be between {min} and {max}"))
    }
}
#[derive(Debug, Args, Default)]
pub struct LocationOptions {
    #[arg(long, requires="lon", allow_hyphen_values=true, value_parser=latitude)]
    pub lat: Option<f64>,
    #[arg(long, requires="lat", allow_hyphen_values=true, value_parser=longitude)]
    pub lon: Option<f64>,
    #[arg(long, requires = "lat")]
    pub place: Option<String>,
    #[arg(long, requires = "lat")]
    pub city: Option<String>,
    #[arg(long, requires="lat", value_parser=map_size)]
    pub location_presentation: Option<MapSize>,
}
#[derive(Debug, Args, Default)]
pub struct EntryOptions {
    #[command(flatten)]
    pub text: TextSource,
    #[arg(long, allow_hyphen_values = true)]
    pub title: Option<String>,
    #[arg(long, value_parser=crate::store::parse_date)]
    pub date: Option<f64>,
    #[arg(long)]
    pub journal: Option<String>,
    #[command(flatten)]
    pub location: LocationOptions,
}
#[derive(Debug, Args, Default)]
pub struct MediaOptions {
    #[arg(long)]
    pub no_resize: bool,
    #[arg(long)]
    pub photos_link: bool,
    #[arg(long)]
    pub link_title: Option<String>,
}
#[derive(Debug, Args, Default)]
pub struct MutationOptions {
    #[arg(long)]
    pub live: bool,
    #[arg(long)]
    pub accept_risk: bool,
    #[arg(long)]
    pub dry_run: bool,
}
#[derive(Debug, Args)]
pub struct CreateRequest {
    #[command(flatten)]
    pub entry: EntryOptions,
    #[command(flatten)]
    pub media_options: MediaOptions,
    #[command(flatten)]
    pub safety: MutationOptions,
    #[arg(long)]
    pub bookmark: bool,
    #[arg(long, num_args=1.., value_parser=parse_path)]
    pub media: Vec<PathBuf>,
    #[arg(long, num_args=2, value_parser=parse_path)]
    pub live_photo: Vec<PathBuf>,
    #[arg(long)]
    pub link: Option<String>,
}
#[derive(Debug, Args)]
pub struct EditRequest {
    pub id: i64,
    #[command(flatten)]
    pub entry: EntryOptions,
    #[command(flatten)]
    pub media_options: MediaOptions,
    #[command(flatten)]
    pub safety: MutationOptions,
    #[arg(long, conflicts_with = "no_bookmark")]
    pub bookmark: bool,
    #[arg(long)]
    pub no_bookmark: bool,
    #[arg(long, num_args=1.., value_parser=parse_path)]
    pub add_media: Vec<PathBuf>,
    #[arg(long)]
    pub add_link: Option<String>,
    #[arg(long, num_args=1.., conflicts_with="remove_all_media")]
    pub remove_media: Vec<i64>,
    #[arg(long)]
    pub remove_all_media: bool,
    #[arg(long)]
    pub clear_location: bool,
    #[arg(long)]
    pub force: bool,
}
#[derive(Debug, Args)]
pub struct DeleteRequest {
    pub id: i64,
    #[arg(long)]
    pub hard: bool,
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub safety: MutationOptions,
}
#[derive(Debug, Args)]
pub struct RestoreRequest {
    pub id: i64,
    #[command(flatten)]
    pub safety: MutationOptions,
}
#[derive(Debug, Args)]
pub struct EmptyRequest {
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub safety: MutationOptions,
}
#[derive(Debug, Args)]
pub struct RepairRequest {
    #[arg(long, default_value="small",value_parser=map_size)]
    pub to: MapSize,
    #[command(flatten)]
    pub safety: MutationOptions,
}
fn parse_path(s: &str) -> std::result::Result<PathBuf, String> {
    Ok(expand(s))
}
pub fn expand(s: &str) -> PathBuf {
    if s == "~" {
        crate::store::home()
    } else if let Some(s) = s.strip_prefix("~/") {
        crate::store::home().join(s)
    } else {
        s.into()
    }
}

#[cfg(test)]
#[path = "../tests/unit/cli.rs"]
mod tests;
