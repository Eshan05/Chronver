// SPDX-FileCopyrightText: 2026 Eshan05
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use releasescout_engine::{
    Channel, Engine, Selection, catalog,
    http::{CacheOptions, DEFAULT_CACHE_MIB, HttpClient, prune_cache},
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "chronver",
    bin_name = "chronver",
    version,
    about = "Fetch authored release notes; never installs or upgrades software"
)]
struct Cli {
    #[arg(long, global = true)]
    catalog: Option<PathBuf>,
    /// Validate and apply a local JSON source overlay for known products.
    #[arg(long, global = true)]
    catalog_overrides: Option<PathBuf>,
    #[arg(long, global = true, default_value = ".releasescout-cache")]
    cache_dir: PathBuf,
    #[arg(long, global = true, conflicts_with = "refresh")]
    offline: bool,
    #[arg(long, global = true)]
    refresh: bool,
    #[arg(long, global = true)]
    include_inactive: bool,
    #[arg(long, global = true)]
    output: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Manage downloaded HTTP responses without touching saved notes or inventory.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// Run bounded, read-only JSON queries with ordered, isolated per-query results.
    Batch {
        /// JSON request file, or - for stdin (schema_version 1).
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u8).range(1..=8))]
        jobs: u8,
        #[arg(long, default_value_t = 20)]
        note_limit: usize,
        #[arg(long, default_value_t = 0)]
        note_offset: usize,
        #[arg(long, default_value_t = 16384)]
        body_bytes: usize,
        #[arg(long, default_value_t = 0)]
        body_offset_bytes: usize,
        /// Total returned Markdown body bytes across the batch, excluding JSON metadata.
        #[arg(long, default_value_t = 65536)]
        body_budget_bytes: usize,
    },
    Scan {
        #[arg(long)]
        manager: releasescout_engine::inventory::Manager,
        #[arg(long)]
        check_updates: bool,
        #[arg(long)]
        show_plan: bool,
    },
    History {
        id: String,
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        cursor: Option<PathBuf>,
    },
    Catalog,
    Range {
        id: String,
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, value_enum)]
        channel: Option<CliChannel>,
    },
    Updates {
        id: String,
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        since: NaiveDate,
        #[arg(long)]
        until: NaiveDate,
    },
    Inspect {
        id: String,
        #[arg(long)]
        component: Option<String>,
    },
}
#[derive(Subcommand)]
enum CacheCommand {
    /// Preview removal of the oldest raw HTTP responses above a size limit.
    Prune {
        /// Target size in MiB; 0 selects all verified raw HTTP responses.
        #[arg(long, default_value_t = DEFAULT_CACHE_MIB)]
        max_mib: u32,
        /// Remove the selected responses instead of only previewing the plan.
        #[arg(long)]
        apply: bool,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum CliChannel {
    Stable,
    Preview,
    All,
}
impl From<CliChannel> for Channel {
    fn from(value: CliChannel) -> Self {
        match value {
            CliChannel::Stable => Self::Stable,
            CliChannel::Preview => Self::Preview,
            CliChannel::All => Self::All,
        }
    }
}
fn run(cli: Cli) -> releasescout_engine::Result<bool> {
    if let Command::Cache {
        command: CacheCommand::Prune { max_mib, apply },
    } = cli.command
    {
        let report = prune_cache(&cli.cache_dir, max_mib, apply)?;
        write_json(cli.output, serde_json::to_string_pretty(&report)?)?;
        return Ok(!report.errors.is_empty());
    }
    let mut catalog = match cli.catalog {
        Some(path) => catalog::load(&std::fs::read_to_string(path)?)?,
        None => catalog::bundled()?,
    };
    if let Some(path) = cli.catalog_overrides {
        catalog::apply_source_overrides(&mut catalog, &path)?;
    }
    let client = HttpClient::new(CacheOptions {
        directory: cli.cache_dir,
        offline: cli.offline,
        refresh: cli.refresh,
        github_token: std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty()),
        ..Default::default()
    })?;
    let engine = Engine {
        catalog,
        fetcher: client,
    };
    let mut failed = false;
    let json = match cli.command {
        Command::Batch {
            input,
            jobs,
            note_limit,
            note_offset,
            body_bytes,
            body_offset_bytes,
            body_budget_bytes,
        } => {
            let request = if input.as_os_str() == "-" {
                releasescout_engine::batch::read_request(std::io::stdin().lock())?
            } else {
                releasescout_engine::batch::read_request(std::fs::File::open(input)?)?
            };
            let response = releasescout_engine::batch::run(
                &engine,
                request,
                jobs as usize,
                releasescout_engine::batch::Limits {
                    note_limit,
                    note_offset,
                    body_bytes,
                    body_offset_bytes,
                    body_budget_bytes,
                },
                cli.include_inactive,
            )?;
            failed = response.failed();
            serde_json::to_string(&response)?
        }
        Command::Scan {
            manager,
            check_updates,
            show_plan,
        } => {
            if !show_plan && (cli.offline || cli.refresh) {
                return Err(releasescout_engine::Error::Query("note-cache flags do not apply to inventory scans; manager-owned network/cache behavior is separate".into()));
            }
            if show_plan {
                serde_json::to_string_pretty(
                    &serde_json::json!({"manager":manager,"script":releasescout_engine::inventory::script(manager,check_updates)}),
                )?
            } else {
                serde_json::to_string_pretty(&releasescout_engine::inventory::scan(
                    manager,
                    check_updates,
                    &releasescout_engine::inventory::ProcessRunner::default(),
                )?)?
            }
        }
        Command::History {
            id,
            component,
            cursor,
        } => {
            let cursor = cursor
                .map(
                    |path| -> releasescout_engine::Result<releasescout_engine::HistoryCursor> {
                        let data: serde_json::Value =
                            serde_json::from_str(&std::fs::read_to_string(path)?)?;
                        Ok(serde_json::from_value(
                            data.get("next").cloned().unwrap_or(data),
                        )?)
                    },
                )
                .transpose()?;
            serde_json::to_string_pretty(&engine.history(
                &id,
                component.as_deref(),
                cursor,
                cli.include_inactive,
            )?)?
        }
        Command::Catalog => serde_json::to_string_pretty(&engine.catalog)?,
        command => {
            let (id, component, selection) = match command {
                Command::Range {
                    id,
                    component,
                    from,
                    to,
                    channel,
                } => {
                    let default = engine
                        .stream(engine.package(&id)?, component.as_deref())?
                        .channel;
                    (
                        id,
                        component,
                        Selection::Versions {
                            from,
                            to,
                            channel: channel.map(Into::into).unwrap_or(default),
                        },
                    )
                }
                Command::Updates {
                    id,
                    component,
                    since,
                    until,
                } => (id, component, Selection::Dates { since, until }),
                Command::Inspect { id, component } => (id, component, Selection::Inspect),
                Command::Catalog
                | Command::Cache { .. }
                | Command::Batch { .. }
                | Command::Scan { .. }
                | Command::History { .. } => unreachable!(),
            };
            let report =
                engine.notes(&id, component.as_deref(), selection, cli.include_inactive)?;
            failed = report.status == "source_failed";
            serde_json::to_string_pretty(&report)?
        }
    };
    write_json(cli.output, json)?;
    Ok(failed)
}

fn write_json(output: Option<PathBuf>, json: String) -> releasescout_engine::Result<()> {
    if let Some(path) = output {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
    } else {
        println!("{json}");
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run(Cli::parse()) {
        Ok(false) => std::process::ExitCode::SUCCESS,
        Ok(true) => std::process::ExitCode::from(2),
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(1)
        }
    }
}
