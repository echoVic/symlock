use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::model::{Lock, Symbol};
use crate::store::{now_ms, Store};
use crate::symbols::{extract_symbols, Lang};

#[derive(Parser)]
#[command(
    name = "symlock",
    about = "Symbol-level locking & conflict prevention for parallel AI coding agents",
    version
)]
pub struct Cli {
    /// Emit machine-readable JSON (for orchestrators / MCP bridges).
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a `.symlock/` store in the current directory.
    Init,
    /// List the lockable symbols (functions/classes/methods) in a file.
    Symbols { file: PathBuf },
    /// Claim a symbol for an agent. Fails (exit 2) if it overlaps an existing claim.
    Claim {
        /// Agent identifier (e.g. "codex-1", "claude-auth").
        #[arg(long)]
        agent: String,
        file: PathBuf,
        /// Symbol name to claim (as reported by `symbols`).
        symbol: String,
    },
    /// Release claims. Without --symbol, releases all of the agent's claims.
    Release {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        symbol: Option<String>,
    },
    /// Show all active claims.
    Status,
}

/// Exit codes are part of the contract so orchestrators can branch on them.
const EXIT_CONFLICT: u8 = 2;
const EXIT_ERROR: u8 = 1;

pub fn run(cli: Cli) -> ExitCode {
    let result = match &cli.command {
        Command::Init => cmd_init(cli.json),
        Command::Symbols { file } => cmd_symbols(file, cli.json),
        Command::Claim {
            agent,
            file,
            symbol,
        } => return cmd_claim(agent, file, symbol, cli.json),
        Command::Release {
            agent,
            file,
            symbol,
        } => cmd_release(agent, file.as_deref(), symbol.as_deref(), cli.json),
        Command::Status => cmd_status(cli.json),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            emit_error(&e, cli.json);
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn cmd_init(json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let dir = Store::init(&cwd)?;
    if json {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "created": dir.to_string_lossy() })
        );
    } else {
        println!("initialized {}", dir.display());
    }
    Ok(())
}

/// Resolve a file to its language + symbols, or a clear error.
fn symbols_for(file: &Path) -> Result<(Lang, Vec<Symbol>)> {
    let lang = Lang::from_path(file)
        .with_context(|| format!("unsupported file type: {}", file.display()))?;
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;
    let symbols = extract_symbols(lang, &source)?;
    Ok((lang, symbols))
}

fn cmd_symbols(file: &Path, json: bool) -> Result<()> {
    let (_, symbols) = symbols_for(file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&symbols)?);
    } else if symbols.is_empty() {
        println!("(no lockable symbols found)");
    } else {
        for s in &symbols {
            println!(
                "{:<8} {:<32} L{}-{}",
                s.kind, s.name, s.start_line, s.end_line
            );
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct ConflictReport<'a> {
    ok: bool,
    conflict: bool,
    requested: &'a Symbol,
    /// Existing claims whose line range overlaps the requested symbol.
    conflicts_with: Vec<&'a Lock>,
}

fn cmd_claim(agent: &str, file: &Path, symbol_name: &str, json: bool) -> ExitCode {
    let (_, symbols) = match symbols_for(file) {
        Ok(v) => v,
        Err(e) => {
            emit_error(&e, json);
            return ExitCode::from(EXIT_ERROR);
        }
    };

    let target = match symbols.iter().find(|s| s.name == symbol_name) {
        Some(s) => s.clone(),
        None => {
            let e = anyhow::anyhow!(
                "symbol `{}` not found in {} (try `symlock symbols`)",
                symbol_name,
                file.display()
            );
            emit_error(&e, json);
            return ExitCode::from(EXIT_ERROR);
        }
    };

    let rel = normalize_path(file);
    let store = match Store::discover(&std::env::current_dir().unwrap_or_default()) {
        Ok(s) => s,
        Err(e) => {
            emit_error(&e, json);
            return ExitCode::from(EXIT_ERROR);
        }
    };

    // Conflict check + insert happen inside one exclusive section so two agents
    // can't both pass the check and then both write.
    let outcome = store.with_exclusive(|lf| {
        // Overlap = same file AND intersecting line ranges, held by a *different* agent.
        let conflicts: Vec<Lock> = lf
            .locks
            .iter()
            .filter(|l| {
                l.file == rel
                    && l.agent != agent
                    && ranges_overlap(l.start_line, l.end_line, target.start_line, target.end_line)
            })
            .cloned()
            .collect();

        if !conflicts.is_empty() {
            return Ok(Err(conflicts));
        }

        // Idempotent: drop any prior claim by this agent on the same symbol, then insert.
        lf.locks
            .retain(|l| !(l.file == rel && l.agent == agent && l.symbol == target.name));
        lf.locks.push(Lock {
            agent: agent.to_string(),
            file: rel.clone(),
            symbol: target.name.clone(),
            kind: target.kind.clone(),
            start_line: target.start_line,
            end_line: target.end_line,
            claimed_at_ms: now_ms(),
        });
        Ok(Ok(()))
    });

    match outcome {
        Ok(Ok(())) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "conflict": false, "claimed": target.name })
                );
            } else {
                println!("claimed {} ({}) for {}", target.name, rel, agent);
            }
            ExitCode::SUCCESS
        }
        Ok(Err(conflicts)) => {
            let refs: Vec<&Lock> = conflicts.iter().collect();
            let report = ConflictReport {
                ok: false,
                conflict: true,
                requested: &target,
                conflicts_with: refs,
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!(
                    "CONFLICT: {} (L{}-{}) overlaps an active claim:",
                    target.name, target.start_line, target.end_line
                );
                for c in &conflicts {
                    eprintln!(
                        "  - {} held by {} (L{}-{})",
                        c.symbol, c.agent, c.start_line, c.end_line
                    );
                }
            }
            ExitCode::from(EXIT_CONFLICT)
        }
        Err(e) => {
            emit_error(&e, json);
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn cmd_release(
    agent: &str,
    file: Option<&Path>,
    symbol: Option<&str>,
    json: bool,
) -> Result<()> {
    let rel = file.map(normalize_path);
    let store = Store::discover(&std::env::current_dir()?)?;
    let removed = store.with_exclusive(|lf| {
        let before = lf.locks.len();
        lf.locks.retain(|l| {
            let same_agent = l.agent == agent;
            let same_file = rel.as_ref().map(|r| &l.file == r).unwrap_or(true);
            let same_symbol = symbol.map(|s| l.symbol == s).unwrap_or(true);
            // Keep everything that is NOT (this agent's matching claim).
            !(same_agent && same_file && same_symbol)
        });
        Ok(before - lf.locks.len())
    })?;
    if json {
        println!("{}", serde_json::json!({ "ok": true, "released": removed }));
    } else {
        println!("released {removed} claim(s)");
    }
    Ok(())
}

fn cmd_status(json: bool) -> Result<()> {
    let store = Store::discover(&std::env::current_dir()?)?;
    let lf = store.read()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&lf)?);
    } else if lf.locks.is_empty() {
        println!("(no active claims)");
    } else {
        for l in &lf.locks {
            println!(
                "{:<16} {:<24} {} (L{}-{})",
                l.agent, l.symbol, l.file, l.start_line, l.end_line
            );
        }
    }
    Ok(())
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start <= b_end && b_start <= a_end
}

/// Normalize to a stable repo-relative-ish POSIX key. We keep it simple for the MVP:
/// strip a leading `./` and convert separators. (Full repo-root resolution is future work.)
fn normalize_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

fn emit_error(e: &anyhow::Error, json: bool) {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({ "ok": false, "error": e.to_string() })
        );
    } else {
        eprintln!("error: {e:#}");
    }
}
