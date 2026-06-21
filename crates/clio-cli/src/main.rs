use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, Subcommand};
use tracing::error;

use clio_core::assembly::{self, ContextBrief, ContextPreset, ContextRequest};
use clio_core::capture::CaptureResult;
use clio_core::config::resolve_db_path;
use clio_core::context;
use clio_core::daemon::{self, DaemonStatus, HealthStatus};
use clio_core::db;
use clio_core::embeddings;
use clio_core::export;
use clio_core::models::{
    LinkInput, Memory, MemoryLink, MemoryStats, RecallQuery, RecallResult, RecentEntry,
    RememberInput, SortOrder,
};
use clio_core::repository;
use clio_core::review;
use clio_core::settings;
use clio_core::stats;

// ---------------------------------------------------------------------------
// CLI argument definitions
// ---------------------------------------------------------------------------

/// Clio -- local-first shared memory for AI tooling.
#[derive(Parser)]
#[command(name = "clio", version, about)]
struct Cli {
    /// Override the database file path.
    #[arg(long, global = true)]
    db_path: Option<String>,

    /// Output as JSON instead of human-readable text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialise the database (optionally create a .clio-namespace file).
    Init(InitArgs),

    /// Show the detected namespace context for the current directory.
    Context,

    /// Store a memory.
    Remember(RememberArgs),

    /// Search or filter memories (full-text).
    Recall(RecallArgs),

    /// Show a single memory by ID.
    Show {
        /// The memory ID to display.
        id: String,
    },

    /// List recent memories.
    Recent(RecentArgs),

    /// Soft-archive a memory.
    Archive {
        /// The memory ID to archive.
        id: String,
    },

    /// Restore an archived memory.
    Unarchive {
        /// The memory ID to unarchive.
        id: String,
    },

    /// Move memories to a different namespace.
    Move(MoveArgs),

    /// Permanently delete a memory by ID.
    Delete {
        /// The memory ID to delete.
        id: String,
    },

    /// Find and optionally purge stale namespaces (dry-run by default).
    Cleanup(CleanupArgs),

    /// Roll a namespace's memories into a single AI-curated consolidated memory.
    Consolidate(ConsolidateArgs),

    /// List all namespaces in use.
    Namespaces,

    /// Create a link between two memories.
    Link(LinkArgs),

    /// Export memories to JSONL.
    Export(ExportArgs),

    /// Import memories from JSONL.
    Import(ImportArgs),

    /// Show database schema summary.
    Schema,

    /// Semantic search — find memories by meaning.
    Search(SearchArgs),

    /// Manage vector embeddings.
    Embed(EmbedArgs),

    /// Capture unstructured text: classify via LLM and store as a memory.
    Capture(CaptureArgs),

    /// Distil a long body of text (e.g. a session transcript) into zero or
    /// more durable memories via LLM.
    Distill(DistillArgs),

    /// Manage the review queue (low-confidence captures).
    Inbox(InboxArgs),

    /// Show memory statistics and analytics.
    Stats(StatsArgs),

    /// Show recent memory activity (creates, updates, archives).
    Activity(ActivityArgs),

    /// Suggest potential links based on embedding similarity.
    SuggestLinks(SuggestLinksArgs),

    /// Import memories from other AI tools (Claude, ChatGPT).
    Migrate(MigrateArgs),

    /// View or update Clio settings.
    Settings(SettingsArgs),

    /// Build a context brief for agent consumption.
    Brief(BriefArgs),

    /// Start the MCP server (stdio transport).
    Serve,

    /// Generate MCP client configuration for a specific AI tool.
    Setup(SetupArgs),

    /// Manage the Clio daemon.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },

    /// Manage the in-memory cache (long-running processes only).
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

#[derive(Parser)]
struct InitArgs {
    /// Create a .clio-namespace file with this namespace in the current directory.
    #[arg(long)]
    namespace: Option<String>,
}

#[derive(Parser)]
struct RememberArgs {
    /// Namespace for the memory. Auto-detected from cwd if omitted.
    #[arg(long)]
    namespace: Option<String>,

    /// Kind of memory (e.g. note, decision, snippet).
    #[arg(long, default_value = "note")]
    kind: String,

    /// Short title for the memory.
    #[arg(long)]
    title: Option<String>,

    /// Brief summary.
    #[arg(long)]
    summary: Option<String>,

    /// Content body. Pass `-` to read from stdin.
    #[arg(long)]
    content: String,

    /// Comma-separated tags.
    #[arg(long)]
    tags: Option<String>,

    /// Source identifier (e.g. tool name, file path).
    #[arg(long)]
    source: Option<String>,

    /// Source-specific reference (e.g. line number, URL).
    #[arg(long)]
    source_ref: Option<String>,

    /// Confidence score (0.0 to 1.0).
    #[arg(long, value_parser = parse_confidence)]
    confidence: Option<f64>,

    /// Importance level (1-5).
    #[arg(long, default_value_t = 3, value_parser = parse_importance)]
    importance: i32,

    /// Arbitrary metadata as a JSON string.
    #[arg(long)]
    metadata: Option<String>,

    /// Upsert: update an existing memory matched by source + source_ref.
    #[arg(long)]
    upsert: bool,
}

#[derive(Parser)]
struct RecallArgs {
    /// Full-text search query.
    #[arg(long)]
    query: Option<String>,

    /// Filter by namespace.
    #[arg(long)]
    namespace: Option<String>,

    /// Search across all namespaces.
    #[arg(long, short = 'g', conflicts_with = "namespace")]
    global: bool,

    /// Filter by kind.
    #[arg(long)]
    kind: Option<String>,

    /// Comma-separated tags to filter by.
    #[arg(long)]
    tags: Option<String>,

    /// Match ANY tag instead of ALL tags.
    #[arg(long)]
    match_any: bool,

    /// Minimum importance (1–5).
    #[arg(long)]
    importance_min: Option<i32>,

    /// Maximum importance (1–5).
    #[arg(long)]
    importance_max: Option<i32>,

    /// Sort order: updated-desc, updated-asc, importance-desc, importance-asc, created-desc, created-asc.
    #[arg(long)]
    sort: Option<String>,

    /// Include archived memories in results.
    #[arg(long)]
    include_archived: bool,

    /// Maximum number of results.
    #[arg(long, default_value_t = 10)]
    limit: u32,

    /// Offset for pagination.
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Parser)]
struct RecentArgs {
    /// Filter by namespace.
    #[arg(long)]
    namespace: Option<String>,

    /// Show recent memories across all namespaces.
    #[arg(long, short = 'g', conflicts_with = "namespace")]
    global: bool,

    /// Filter by kind.
    #[arg(long)]
    kind: Option<String>,

    /// Comma-separated tags to filter by.
    #[arg(long)]
    tags: Option<String>,

    /// Match ANY tag instead of ALL tags.
    #[arg(long)]
    match_any: bool,

    /// Minimum importance (1–5).
    #[arg(long)]
    importance_min: Option<i32>,

    /// Maximum importance (1–5).
    #[arg(long)]
    importance_max: Option<i32>,

    /// Sort order: updated-desc, updated-asc, importance-desc, importance-asc, created-desc, created-asc.
    #[arg(long)]
    sort: Option<String>,

    /// Include archived memories in results.
    #[arg(long)]
    include_archived: bool,

    /// Maximum number of results.
    #[arg(long, default_value_t = 10)]
    limit: u32,
}

#[derive(Parser)]
struct MoveArgs {
    /// Memory ID to move. Omit to move all memories in --from namespace.
    #[arg(long)]
    id: Option<String>,

    /// Source namespace (move all memories from this namespace). Ignored if --id is given.
    #[arg(long)]
    from: Option<String>,

    /// Target namespace.
    #[arg(long)]
    to: String,
}

#[derive(Parser)]
struct CleanupArgs {
    /// Flag namespaces with no activity for this many months (default from settings).
    #[arg(long)]
    stale_months: Option<u32>,

    /// Flag namespaces whose memories are all archived.
    #[arg(long)]
    archived: bool,

    /// Flag project namespaces whose folder is no longer on disk.
    #[arg(long)]
    folder_gone: bool,

    /// Apply all criteria (stale + archived + folder-gone). This is the default
    /// when no specific criterion flag is given.
    #[arg(long)]
    all: bool,

    /// Actually purge the candidates. Without this it is a dry run. A database
    /// backup is always taken before purging.
    #[arg(long)]
    execute: bool,
}

#[derive(Parser)]
struct ConsolidateArgs {
    /// Namespace to consolidate. Auto-detected from the working directory if omitted.
    #[arg(long)]
    namespace: Option<String>,

    /// Consolidate every namespace (overrides --namespace).
    #[arg(long)]
    all: bool,

    /// Only consolidate a namespace if it has accrued enough new memories since
    /// its last consolidation (the configured auto_threshold). No-op otherwise.
    #[arg(long)]
    if_due: bool,
}

#[derive(Parser)]
struct LinkArgs {
    /// Source memory ID.
    from: String,

    /// Target memory ID.
    to: String,

    /// Relationship label.
    #[arg(long, default_value = "relates_to")]
    relationship: String,

    /// Arbitrary metadata as a JSON string.
    #[arg(long)]
    metadata: Option<String>,
}

#[derive(Parser)]
struct ExportArgs {
    /// Output file path, or `-` for stdout.
    #[arg(long)]
    output: String,

    /// Filter export by namespace.
    #[arg(long)]
    namespace: Option<String>,

    /// Include archived memories.
    #[arg(long)]
    include_archived: bool,
}

#[derive(Parser)]
struct ImportArgs {
    /// Input file path, or `-` for stdin.
    #[arg(long)]
    input: String,
}

#[derive(Parser)]
struct SearchArgs {
    /// The natural language query to search for.
    query: String,

    /// Filter by namespace.
    #[arg(long)]
    namespace: Option<String>,

    /// Search across all namespaces.
    #[arg(long, short = 'g', conflicts_with = "namespace")]
    global: bool,

    /// Include archived memories in results.
    #[arg(long)]
    include_archived: bool,

    /// Maximum number of results.
    #[arg(long, default_value_t = 10)]
    limit: u32,
}

#[derive(Parser)]
struct CaptureArgs {
    /// The unstructured text to capture. Pass `-` to read from stdin.
    text: String,

    /// Override the namespace suggested by the LLM.
    #[arg(long)]
    namespace: Option<String>,

    /// Show classification without storing the memory.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Parser)]
struct DistillArgs {
    /// The text to distil (e.g. a session transcript). Pass `-` to read from stdin.
    text: String,

    /// Override the namespace suggested by the LLM for every distilled memory.
    #[arg(long)]
    namespace: Option<String>,

    /// Provenance source recorded on each distilled memory.
    #[arg(long, default_value = "distill")]
    source: String,

    /// Provenance source_ref recorded on each distilled memory (e.g. session id).
    #[arg(long)]
    source_ref: Option<String>,

    /// Show the distilled memories without storing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Parser)]
struct StatsArgs {
    /// Scope statistics to a specific namespace.
    #[arg(long)]
    namespace: Option<String>,
}

#[derive(Parser)]
struct ActivityArgs {
    /// Filter activity to a specific namespace.
    #[arg(long)]
    namespace: Option<String>,

    /// Maximum number of activity entries to show.
    #[arg(long, default_value_t = 20)]
    limit: u32,
}

#[derive(Parser)]
struct BriefArgs {
    /// Namespace scope. Auto-detected from cwd if omitted.
    #[arg(long)]
    namespace: Option<String>,

    /// Preset: project-brief, person-brief, decision-history, active-constraints, recent-activity, custom.
    #[arg(long, default_value = "project-brief")]
    preset: String,

    /// FTS query (used with --preset custom).
    #[arg(long)]
    query: Option<String>,

    /// Maximum memories to include.
    #[arg(long, default_value_t = 20)]
    max_items: u32,

    /// Include linked memories.
    #[arg(long)]
    include_links: bool,
}

#[derive(Parser)]
struct SuggestLinksArgs {
    /// The memory ID to find link suggestions for.
    memory_id: String,

    /// Minimum similarity threshold (0.0 to 1.0).
    #[arg(long, default_value_t = 0.7)]
    threshold: f64,

    /// Maximum number of suggestions.
    #[arg(long, default_value_t = 5)]
    limit: u32,
}

#[derive(Parser)]
struct InboxArgs {
    #[command(subcommand)]
    command: InboxSubcommand,
}

#[derive(Subcommand)]
enum InboxSubcommand {
    /// List pending review items.
    List {
        /// Maximum number of items to show.
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },

    /// Approve a review item (converts to a memory).
    Approve {
        /// The review item ID.
        id: String,
    },

    /// Reject a review item.
    Reject {
        /// The review item ID.
        id: String,
    },

    /// Edit a review item's suggested fields before approval.
    Edit {
        /// The review item ID.
        id: String,

        /// Override suggested title.
        #[arg(long)]
        title: Option<String>,

        /// Override suggested namespace.
        #[arg(long)]
        namespace: Option<String>,

        /// Override suggested kind.
        #[arg(long)]
        kind: Option<String>,

        /// Override suggested tags (comma-separated).
        #[arg(long)]
        tags: Option<String>,

        /// Override suggested summary.
        #[arg(long)]
        summary: Option<String>,

        /// Override suggested importance (1-5).
        #[arg(long)]
        importance: Option<i32>,
    },

    /// Show review queue statistics.
    Stats,
}

#[derive(Parser)]
struct MigrateArgs {
    #[command(subcommand)]
    command: MigrateSubcommand,
}

#[derive(Subcommand)]
enum MigrateSubcommand {
    /// Import memories from a Claude memory export.
    Claude {
        /// Path to the Claude export file (text or JSON).
        file: String,

        /// Override the namespace for all imported memories.
        #[arg(long)]
        namespace: Option<String>,

        /// Run each entry through the capture pipeline for richer classification.
        #[arg(long)]
        classify: bool,

        /// Show what would be imported without storing anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Import memories from a ChatGPT memory export.
    Chatgpt {
        /// Path to the ChatGPT export file (JSON).
        file: String,

        /// Override the namespace for all imported memories.
        #[arg(long)]
        namespace: Option<String>,

        /// Run each entry through the capture pipeline for richer classification.
        #[arg(long)]
        classify: bool,

        /// Show what would be imported without storing anything.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum EmbedSubcommand {
    /// Show embedding status (how many memories have/lack embeddings).
    Status,

    /// Generate embeddings for all memories that don't have them yet.
    Backfill {
        /// Maximum number of memories to embed in this run.
        #[arg(long, default_value_t = 100)]
        batch_size: u32,
    },
}

#[derive(Parser)]
struct EmbedArgs {
    #[command(subcommand)]
    command: EmbedSubcommand,
}

#[derive(Subcommand)]
enum SettingsSubcommand {
    /// Show current settings.
    Show,

    /// Set the embedding provider to "local" (fastembed, offline).
    UseLocal,

    /// Set the embedding provider to "openai".
    UseOpenai {
        /// OpenAI API key. If omitted, OPENAI_API_KEY env var will be used at runtime.
        #[arg(long)]
        api_key: Option<String>,

        /// Model to use. Default: text-embedding-3-small.
        #[arg(long, default_value = "text-embedding-3-small")]
        model: String,

        /// Optional base URL override.
        #[arg(long)]
        base_url: Option<String>,
    },

    /// Disable embeddings entirely.
    Disable,

    /// Enable the capture pipeline with an OpenAI-compatible API.
    UseCapture {
        /// API key for the LLM provider.
        #[arg(long)]
        api_key: String,

        /// Model to use for classification. Default: gpt-4o-mini.
        #[arg(long, default_value = "gpt-4o-mini")]
        model: String,

        /// Base URL override for compatible APIs.
        #[arg(long)]
        base_url: Option<String>,
    },

    /// Disable the capture pipeline.
    DisableCapture,
}

#[derive(Parser)]
struct SettingsArgs {
    #[command(subcommand)]
    command: SettingsSubcommand,
}

#[derive(Parser)]
struct SetupArgs {
    /// The AI client to generate configuration for.
    #[command(subcommand)]
    client: SetupClient,

    /// Preview the configuration without writing it.
    #[arg(long, global = true)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum SetupClient {
    /// Install MCP config for Claude Code / Claude Desktop.
    ClaudeCode,

    /// Install MCP config for Cursor.
    Cursor,

    /// Install MCP config for Windsurf.
    Windsurf,

    /// Install MCP config for OpenAI Codex CLI.
    Codex,

    /// Install MCP config for OpenCode.
    Opencode,

    /// Install MCP config for Kilo Code.
    Kilo,

    /// Install MCP config for Kimi Code CLI.
    Kimi,

    /// Install MCP config for GitHub Copilot CLI.
    Copilot,

    /// Install MCP config for Google Gemini CLI.
    Gemini,

    /// Generate a generic MCP config snippet.
    Generic,
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Start the daemon in the foreground.
    Run,

    /// Start the daemon in the background.
    Start,

    /// Stop the running daemon.
    Stop,

    /// Restart the daemon.
    Restart,

    /// Show daemon status.
    Status,

    /// Tail daemon logs.
    Logs {
        /// Number of lines to show.
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
    },

    /// Install the daemon as a macOS LaunchAgent.
    Install,

    /// Uninstall the daemon LaunchAgent.
    Uninstall,

    /// Run health checks against the local database and configuration.
    Doctor,
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Clear all cached data.
    Clear,
    /// Show cache statistics.
    Stats,
}

// ---------------------------------------------------------------------------
// Clap value parsers
// ---------------------------------------------------------------------------

fn parse_confidence(s: &str) -> Result<f64, String> {
    let val: f64 = s.parse().map_err(|e| format!("invalid confidence: {e}"))?;
    if !(0.0..=1.0).contains(&val) {
        return Err("confidence must be between 0.0 and 1.0".into());
    }
    Ok(val)
}

fn parse_importance(s: &str) -> Result<i32, String> {
    let val: i32 = s.parse().map_err(|e| format!("invalid importance: {e}"))?;
    if !(1..=5).contains(&val) {
        return Err("importance must be between 1 and 5".into());
    }
    Ok(val)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();

    if let Err(err) = run(cli) {
        error!("{err}");
        eprintln!("Error: {err}");
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Init(args) => cmd_init(cli.db_path.as_deref(), args),
        Command::Context => cmd_context(cli.json),
        Command::Remember(args) => cmd_remember(cli.db_path.as_deref(), cli.json, args),
        Command::Recall(args) => cmd_recall(cli.db_path.as_deref(), cli.json, args),
        Command::Show { id } => cmd_show(cli.db_path.as_deref(), cli.json, &id),
        Command::Recent(args) => cmd_recent(cli.db_path.as_deref(), cli.json, args),
        Command::Archive { id } => cmd_archive(cli.db_path.as_deref(), cli.json, &id),
        Command::Unarchive { id } => cmd_unarchive(cli.db_path.as_deref(), cli.json, &id),
        Command::Move(args) => cmd_move(cli.db_path.as_deref(), cli.json, args),
        Command::Delete { id } => cmd_delete(cli.db_path.as_deref(), cli.json, &id),
        Command::Cleanup(args) => cmd_cleanup(cli.db_path.as_deref(), cli.json, args),
        Command::Consolidate(args) => cmd_consolidate(cli.db_path.as_deref(), cli.json, args),
        Command::Namespaces => cmd_namespaces(cli.db_path.as_deref(), cli.json),
        Command::Link(args) => cmd_link(cli.db_path.as_deref(), cli.json, args),
        Command::Export(args) => cmd_export(cli.db_path.as_deref(), args),
        Command::Import(args) => cmd_import(cli.db_path.as_deref(), args),
        Command::Schema => cmd_schema(cli.db_path.as_deref(), cli.json),
        Command::Search(args) => cmd_search(cli.db_path.as_deref(), cli.json, args),
        Command::Capture(args) => cmd_capture(cli.db_path.as_deref(), cli.json, args),
        Command::Distill(args) => cmd_distill(cli.db_path.as_deref(), cli.json, args),
        Command::Inbox(args) => cmd_inbox(cli.db_path.as_deref(), cli.json, args),
        Command::Stats(args) => cmd_stats(cli.db_path.as_deref(), cli.json, args),
        Command::Activity(args) => cmd_activity(cli.db_path.as_deref(), cli.json, args),
        Command::SuggestLinks(args) => cmd_suggest_links(cli.db_path.as_deref(), cli.json, args),
        Command::Embed(args) => cmd_embed(cli.db_path.as_deref(), args),
        Command::Migrate(args) => cmd_migrate(cli.db_path.as_deref(), cli.json, args),
        Command::Settings(args) => cmd_settings(cli.db_path.as_deref(), cli.json, args),
        Command::Brief(args) => cmd_brief(cli.db_path.as_deref(), cli.json, args),
        Command::Serve => cmd_serve(cli.db_path.as_deref()),
        Command::Setup(args) => cmd_setup(cli.db_path.as_deref(), cli.json, args),
        Command::Daemon { command } => cmd_daemon(cli.db_path.as_deref(), cli.json, command),
        Command::Cache { command } => match command {
            CacheCommand::Clear => {
                println!(
                    "Cache management is only effective in long-running processes (MCP server, Tauri app)."
                );
                println!(
                    "The CLI creates a fresh process for each command, so there is no persistent cache to clear."
                );
                println!("\nTo clear the MCP server cache, use the memory_cache_clear tool.");
                Ok(())
            }
            CacheCommand::Stats => {
                println!(
                    "Cache management is only effective in long-running processes (MCP server, Tauri app)."
                );
                println!(
                    "The CLI creates a fresh process for each command, so there is no persistent cache."
                );
                Ok(())
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Helper: open the database
// ---------------------------------------------------------------------------

fn open_db(explicit: Option<&str>) -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let path = resolve_db_path(explicit)?;
    let conn = db::open(&path)?;
    Ok(conn)
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn cmd_init(db_path: Option<&str>, args: InitArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let _conn = db::open(&path)?;
    eprintln!("Database initialised at {}", path.display());

    let inbox = clio_core::config::ensure_inbox_dir(&path)?;
    eprintln!("Inbox directory at {}", inbox.display());

    if let Some(ref ns) = args.namespace {
        let cwd = std::env::current_dir()?;
        context::init_namespace(&cwd, ns)?;
        eprintln!("Created .clio-namespace with namespace: {ns}");
    }

    Ok(())
}

fn cmd_context(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let detected = context::detect_namespace(&cwd);

    if json {
        let obj = match &detected {
            Some(ctx) => serde_json::json!({
                "namespace": ctx.namespace,
                "source": format!("{}", ctx.source),
                "marker_path": ctx.marker_path,
                "cwd": cwd.display().to_string(),
            }),
            None => serde_json::json!({
                "namespace": "global",
                "source": "default",
                "marker_path": null,
                "cwd": cwd.display().to_string(),
            }),
        };
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        match detected {
            Some(ctx) => {
                eprintln!("Detected context:");
                eprintln!("  namespace:   {}", ctx.namespace);
                eprintln!("  detected by: {}", ctx.source);
                eprintln!("  marker at:   {}", ctx.marker_path);
                eprintln!("  cwd:         {}", cwd.display());
            }
            None => {
                eprintln!("No project context detected.");
                eprintln!("  namespace:   global (default)");
                eprintln!("  cwd:         {}", cwd.display());
                eprintln!();
                eprintln!("Tip: run `clio init --namespace project:my-app` to set a namespace.");
            }
        }
    }

    Ok(())
}

fn cmd_remember(
    db_path: Option<&str>,
    json: bool,
    args: RememberArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    let cwd = std::env::current_dir().ok();
    let namespace = context::resolve_namespace(
        args.namespace.as_deref(),
        cwd.as_deref(),
        s.context.auto_detect,
    );

    let content = if args.content == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        args.content
    };

    let tags: Vec<String> = args
        .tags
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let metadata: serde_json::Value = match args.metadata {
        Some(ref s) => serde_json::from_str(s)?,
        None => serde_json::Value::Object(serde_json::Map::new()),
    };

    let input = RememberInput {
        namespace,
        kind: args.kind,
        title: args.title,
        summary: args.summary,
        content,
        tags,
        source: args.source,
        source_ref: args.source_ref,
        confidence: args.confidence,
        importance: args.importance,
        metadata,
        valid_from: None,
        valid_until: None,
        upsert: args.upsert,
    };

    let memory = repository::remember(&conn, &input, &s)?;

    // Auto-embed if enabled.
    if s.auto_embed {
        match embeddings::create_backend(&s.embeddings) {
            Ok(backend) => {
                if let Err(e) = embeddings::embed_and_store(&conn, backend.as_ref(), &memory) {
                    tracing::warn!("auto-embed failed: {e}");
                    eprintln!("Warning: auto-embed failed: {e}");
                }
            }
            Err(e) => {
                tracing::debug!("embedding backend not available, skipping: {e}");
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&memory)?);
    } else {
        eprintln!("Remembered.");
        print_memory_card(&memory);
    }

    Ok(())
}

fn cmd_recall(
    db_path: Option<&str>,
    json: bool,
    args: RecallArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    let cwd = std::env::current_dir().ok();
    let detected_ns = context::resolve_namespace(
        args.namespace.as_deref(),
        cwd.as_deref(),
        s.context.auto_detect,
    );

    let tags: Vec<String> = args
        .tags
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let sort_by = args.sort.as_deref().and_then(SortOrder::from_str_opt);

    // --global: search all namespaces without scoping.
    // --namespace: search only that exact namespace.
    // Neither: use scoped-then-global recall (project namespace + global fallback).
    let scoring = Some(s.scoring.clone());
    let result = if args.global {
        let query = RecallQuery {
            query: args.query,
            namespace: None,
            kind: args.kind,
            tags,
            match_all_tags: !args.match_any,
            include_archived: args.include_archived,
            include_links: false,
            exclude_expired: false,
            importance_min: args.importance_min,
            importance_max: args.importance_max,
            sort_by: sort_by.clone(),
            limit: args.limit,
            offset: args.offset,
            scoring,
        };
        repository::recall(&conn, &query)?
    } else if args.namespace.is_some() {
        let query = RecallQuery {
            query: args.query,
            namespace: Some(detected_ns),
            kind: args.kind,
            tags,
            match_all_tags: !args.match_any,
            include_archived: args.include_archived,
            include_links: false,
            exclude_expired: false,
            importance_min: args.importance_min,
            importance_max: args.importance_max,
            sort_by: sort_by.clone(),
            limit: args.limit,
            offset: args.offset,
            scoring,
        };
        repository::recall(&conn, &query)?
    } else {
        let query = RecallQuery {
            query: args.query,
            namespace: None,
            kind: args.kind,
            tags,
            match_all_tags: !args.match_any,
            include_archived: args.include_archived,
            include_links: false,
            exclude_expired: false,
            importance_min: args.importance_min,
            importance_max: args.importance_max,
            sort_by,
            limit: args.limit,
            offset: args.offset,
            scoring,
        };
        repository::recall_scoped(&conn, &query, &detected_ns)?
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_recall_result(&result);
    }

    Ok(())
}

fn cmd_show(db_path: Option<&str>, json: bool, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let memory = repository::get(&conn, id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&memory)?);
    } else {
        print_memory_card(&memory);
    }

    Ok(())
}

fn cmd_recent(
    db_path: Option<&str>,
    json: bool,
    args: RecentArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    let tags: Vec<String> = args
        .tags
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let sort_by = args.sort.as_deref().and_then(SortOrder::from_str_opt);

    let query = RecallQuery {
        namespace: args.namespace,
        kind: args.kind,
        tags,
        match_all_tags: !args.match_any,
        importance_min: args.importance_min,
        importance_max: args.importance_max,
        sort_by,
        include_archived: args.include_archived,
        limit: args.limit,
        scoring: Some(s.scoring.clone()),
        ..Default::default()
    };
    let result = repository::recall(&conn, &query)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_recall_result(&result);
    }

    Ok(())
}

fn cmd_archive(
    db_path: Option<&str>,
    json: bool,
    id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let memory = repository::archive(&conn, id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&memory)?);
    } else {
        eprintln!("Archived.");
        print_memory_card(&memory);
    }

    Ok(())
}

fn cmd_unarchive(
    db_path: Option<&str>,
    json: bool,
    id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let memory = repository::unarchive(&conn, id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&memory)?);
    } else {
        eprintln!("Unarchived.");
        print_memory_card(&memory);
    }

    Ok(())
}

fn cmd_move(
    db_path: Option<&str>,
    json: bool,
    args: MoveArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;

    if let Some(ref id) = args.id {
        // Move a single memory.
        let memory = repository::move_namespace(&conn, id, &args.to)?;

        if json {
            println!("{}", serde_json::to_string_pretty(&memory)?);
        } else {
            eprintln!("Moved to namespace '{}'.", args.to);
            print_memory_card(&memory);
        }
    } else if let Some(ref from) = args.from {
        // Bulk move: all memories in one namespace to another.
        let count = repository::move_namespace_bulk(&conn, from, &args.to)?;

        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "from": from,
                    "to": args.to,
                    "count": count,
                }))?
            );
        } else {
            eprintln!("Moved {count} memories from '{from}' to '{}'.", args.to);
        }
    } else {
        return Err("provide either --id or --from to specify what to move.".into());
    }

    Ok(())
}

fn cmd_delete(
    db_path: Option<&str>,
    json: bool,
    id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let memory = repository::delete(&conn, id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&memory)?);
    } else {
        eprintln!("Deleted.");
        print_memory_card(&memory);
    }
    Ok(())
}

fn cmd_cleanup(
    db_path: Option<&str>,
    json: bool,
    args: CleanupArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    // Default to all criteria when no specific criterion flag is given.
    let any_specific = args.archived || args.folder_gone || args.stale_months.is_some();
    let use_all = args.all || !any_specific;

    let criteria = clio_core::cleanup::CleanupCriteria {
        stale_months: if use_all || args.stale_months.is_some() {
            Some(args.stale_months.unwrap_or(s.cleanup.stale_months))
        } else {
            None
        },
        all_archived: use_all || args.archived,
        folder_gone: use_all || args.folder_gone,
    };

    let dev_roots = clio_core::cleanup::expand_dev_roots(&s.cleanup.dev_roots);
    let candidates = clio_core::cleanup::find_candidates_now(&conn, &criteria, &dev_roots)?;

    if !args.execute {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "dry_run": true,
                    "candidates": candidates,
                }))?
            );
        } else if candidates.is_empty() {
            eprintln!("No stale namespaces found.");
        } else {
            eprintln!(
                "Found {} candidate namespace(s) — dry run, pass --execute to purge (a backup is taken first):",
                candidates.len()
            );
            for c in &candidates {
                let reasons: Vec<String> = c.reasons.iter().map(|r| format!("{r:?}")).collect();
                eprintln!(
                    "  {} — {} live / {} archived — {}",
                    c.namespace,
                    c.live_count,
                    c.archived_count,
                    reasons.join(", ")
                );
            }
        }
        return Ok(());
    }

    let namespaces: Vec<String> = candidates.iter().map(|c| c.namespace.clone()).collect();
    let report = clio_core::cleanup::execute_cleanup(&conn, &path, &namespaces, 10)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        eprintln!(
            "Purged {} namespace(s), {} memories.",
            report.namespaces_deleted.len(),
            report.memories_purged
        );
        if let Some(bp) = &report.backup_path {
            eprintln!("Backup: {bp}");
        }
    }
    Ok(())
}

fn cmd_consolidate(
    db_path: Option<&str>,
    json: bool,
    args: ConsolidateArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    // Determine which namespaces to process.
    let namespaces: Vec<String> = if args.all {
        repository::list_namespaces(&conn)?
    } else {
        let ns = match args.namespace {
            Some(ns) => ns,
            None => std::env::current_dir()
                .ok()
                .as_deref()
                .and_then(context::detect_namespace)
                .map(|ctx| ctx.namespace)
                .unwrap_or_else(|| "global".into()),
        };
        vec![ns]
    };

    let threshold = s.consolidate.auto_threshold as usize;
    let mut done = Vec::new();
    let mut skipped = 0usize;

    for ns in &namespaces {
        if args.if_due {
            let new = clio_core::consolidate::new_since_last_consolidation(&conn, ns)?;
            if new < threshold {
                skipped += 1;
                continue;
            }
        }
        match clio_core::consolidate::consolidate(&conn, ns, &s.capture, &s) {
            Ok(result) => done.push((ns.clone(), result.source_count)),
            // With --all/--if-due, an empty namespace just means nothing to do.
            Err(_) if args.all || args.if_due => skipped += 1,
            Err(e) => return Err(e.into()),
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "consolidated": done.iter().map(|(ns, n)| serde_json::json!({"namespace": ns, "source_count": n})).collect::<Vec<_>>(),
                "skipped": skipped,
            }))?
        );
    } else if done.is_empty() {
        eprintln!("Nothing consolidated ({skipped} namespace(s) skipped).");
    } else {
        for (ns, n) in &done {
            eprintln!("Consolidated {n} memories into '{ns}'.");
        }
        if skipped > 0 {
            eprintln!("({skipped} namespace(s) skipped.)");
        }
    }
    Ok(())
}

fn cmd_namespaces(db_path: Option<&str>, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let namespaces = repository::list_namespaces(&conn)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&namespaces)?);
    } else if namespaces.is_empty() {
        println!("No namespaces found.");
    } else {
        for ns in &namespaces {
            println!("  {ns}");
        }
    }

    Ok(())
}

fn cmd_link(
    db_path: Option<&str>,
    json: bool,
    args: LinkArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;

    let metadata: serde_json::Value = match args.metadata {
        Some(ref s) => serde_json::from_str(s)?,
        None => serde_json::Value::Object(serde_json::Map::new()),
    };

    let input = LinkInput {
        from_memory_id: args.from,
        to_memory_id: args.to,
        relationship: args.relationship,
        metadata,
    };

    let link = repository::link(&conn, &input)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&link)?);
    } else {
        print_link(&link);
    }

    Ok(())
}

fn cmd_export(db_path: Option<&str>, args: ExportArgs) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;

    if args.output == "-" {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        let count = export::export_jsonl(
            &conn,
            &mut handle,
            args.namespace.as_deref(),
            args.include_archived,
        )?;
        eprintln!("Exported {count} memories to stdout.");
    } else {
        let mut file = std::fs::File::create(&args.output)?;
        let count = export::export_jsonl(
            &conn,
            &mut file,
            args.namespace.as_deref(),
            args.include_archived,
        )?;
        eprintln!("Exported {count} memories to {}.", args.output);
    }

    Ok(())
}

fn cmd_import(db_path: Option<&str>, args: ImportArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;

    let result = if args.input == "-" {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        export::import_jsonl(&conn, &mut handle)?
    } else {
        let mut file = std::fs::File::open(&args.input)?;
        export::import_jsonl(&conn, &mut file)?
    };

    eprintln!(
        "Import complete: {} imported, {} skipped.",
        result.imported, result.skipped
    );

    if !result.errors.is_empty() {
        eprintln!("Errors:");
        for err in &result.errors {
            eprintln!("  {err}");
        }
    }

    // Auto-embed imported memories if enabled.
    if result.imported > 0 {
        let s = settings::load(&path)?;
        if s.auto_embed {
            match embeddings::create_backend(&s.embeddings) {
                Ok(backend) => {
                    let ids = embeddings::list_unembedded(&conn, result.imported)?;
                    let mut success = 0u32;
                    let mut failed = 0u32;

                    for id in &ids {
                        match repository::get(&conn, id) {
                            Ok(memory) => {
                                match embeddings::embed_and_store(&conn, backend.as_ref(), &memory)
                                {
                                    Ok(()) => success += 1,
                                    Err(e) => {
                                        tracing::warn!("auto-embed failed for {id}: {e}");
                                        failed += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("could not load {id} for embedding: {e}");
                                failed += 1;
                            }
                        }
                    }

                    if success > 0 || failed > 0 {
                        eprintln!("Auto-embed: {success} embedded, {failed} failed.");
                    }
                }
                Err(e) => {
                    tracing::debug!("embedding backend not available, skipping: {e}");
                }
            }
        }
    }

    Ok(())
}

fn cmd_schema(db_path: Option<&str>, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let info = repository::schema_info(&conn)?;

    if json {
        let obj = serde_json::json!({ "schema": info });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("{info}");
    }

    Ok(())
}

fn cmd_search(
    db_path: Option<&str>,
    json: bool,
    args: SearchArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    let backend = embeddings::create_backend(&s.embeddings)?;
    let query_embedding = backend.embed_one(&args.query)?;

    let items = embeddings::semantic_recall(
        &conn,
        &args.query,
        &query_embedding,
        args.namespace.as_deref(),
        args.include_archived,
        args.limit,
    )?;

    let result = RecallResult {
        items: items.clone(),
        total: items.len() as u32,
        limit: args.limit,
        offset: 0,
        count: items.len() as u32,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if result.items.is_empty() {
        println!("No semantically similar memories found.");
    } else {
        println!("Semantic search results ({} found):", result.count);
        println!();
        print_recall_result(&result);
    }

    Ok(())
}

fn cmd_capture(
    db_path: Option<&str>,
    json: bool,
    args: CaptureArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    let text = if args.text == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        args.text
    };

    let classification = clio_core::capture::classify(&text, &s.capture)?;

    if args.dry_run {
        if json {
            println!("{}", serde_json::to_string_pretty(&classification)?);
        } else {
            eprintln!("Dry run — classification result:");
            eprintln!("  kind:       {}", classification.kind);
            eprintln!("  title:      {}", classification.title);
            eprintln!("  summary:    {}", classification.summary);
            eprintln!("  tags:       {}", classification.tags.join(", "));
            eprintln!("  namespace:  {}", classification.namespace);
            eprintln!("  importance: {}", classification.importance);
            eprintln!("  confidence: {:.2}", classification.confidence);
        }
        return Ok(());
    }

    let result = clio_core::capture::capture_with_classification(
        &conn,
        &text,
        &classification,
        args.namespace.as_deref(),
        &s,
    )?;

    match result {
        CaptureResult::Stored(memory) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&memory)?);
            } else {
                eprintln!("Captured.");
                print_memory_card(&memory);
            }
        }
        CaptureResult::Queued(item) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&item)?);
            } else {
                eprintln!("Queued for review (confidence below threshold).");
                print_review_item(&item);
            }
        }
    }

    Ok(())
}

fn cmd_distill(
    db_path: Option<&str>,
    json: bool,
    args: DistillArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    let text = if args.text == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        args.text
    };

    if args.dry_run {
        let memories = clio_core::capture::distill(&text, &s.capture)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&memories)?);
        } else if memories.is_empty() {
            eprintln!("Dry run — nothing durable to distil.");
        } else {
            eprintln!("Dry run — {} memory(ies) distilled:", memories.len());
            for m in &memories {
                eprintln!("  [{}] {} (importance {})", m.kind, m.title, m.importance);
            }
        }
        return Ok(());
    }

    // Record the working directory so namespaces can later be matched to a real
    // path for reliable "folder gone" cleanup.
    let cwd = if s.cleanup.record_cwd {
        std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string())
    } else {
        None
    };

    // Default each memory to the working directory's namespace so a session's
    // memories land in the right project, instead of trusting the LLM's guess.
    // An explicit `--namespace` still wins, and the LLM may still promote a
    // fact to "global".
    let default_namespace = std::env::current_dir()
        .ok()
        .and_then(|p| context::detect_namespace(&p))
        .map(|ctx| ctx.namespace);

    let results = clio_core::capture::distill_and_store(
        &conn,
        &text,
        &s.capture,
        args.namespace.as_deref(),
        default_namespace.as_deref(),
        &args.source,
        args.source_ref.as_deref(),
        cwd.as_deref(),
        &s,
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        eprintln!("Nothing durable to distil.");
    } else {
        let stored = results
            .iter()
            .filter(|r| matches!(r, CaptureResult::Stored(_)))
            .count();
        let queued = results.len() - stored;
        eprintln!("Distilled {stored} stored, {queued} queued for review.");
        for result in &results {
            match result {
                CaptureResult::Stored(memory) => print_memory_card(memory),
                CaptureResult::Queued(item) => print_review_item(item),
            }
        }
    }

    Ok(())
}

fn cmd_inbox(
    db_path: Option<&str>,
    json: bool,
    args: InboxArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;

    match args.command {
        InboxSubcommand::List { limit } => {
            let items = review::list_pending(&conn, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else if items.is_empty() {
                eprintln!("No pending review items.");
            } else {
                eprintln!("{} pending item(s):\n", items.len());
                for item in &items {
                    print_review_item(item);
                    println!();
                }
            }
        }
        InboxSubcommand::Approve { id } => {
            let s = settings::load(&path)?;
            let memory = review::approve_review(&conn, &id, &s)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&memory)?);
            } else {
                eprintln!("Approved — memory created.");
                print_memory_card(&memory);
            }
        }
        InboxSubcommand::Reject { id } => {
            let item = review::reject_review(&conn, &id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&item)?);
            } else {
                eprintln!("Rejected.");
                print_review_item(&item);
            }
        }
        InboxSubcommand::Edit {
            id,
            title,
            namespace,
            kind,
            tags,
            summary,
            importance,
        } => {
            let edits = review::ReviewEdits {
                namespace,
                kind,
                title: title.map(Some),
                summary: summary.map(Some),
                tags: tags.map(|t| t.split(',').map(|s| s.trim().to_string()).collect()),
                importance,
                confidence: None,
            };
            let item = review::edit_review(&conn, &id, &edits)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&item)?);
            } else {
                eprintln!("Updated.");
                print_review_item(&item);
            }
        }
        InboxSubcommand::Stats => {
            let stats = review::review_stats(&conn)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                eprintln!("Review queue statistics:");
                eprintln!("  Pending:  {}", stats.pending);
                eprintln!("  Approved: {}", stats.approved);
                eprintln!("  Rejected: {}", stats.rejected);
                eprintln!("  Edited:   {}", stats.edited);
                eprintln!("  Total:    {}", stats.total);
            }
        }
    }

    Ok(())
}

fn cmd_migrate(
    db_path: Option<&str>,
    json: bool,
    args: MigrateArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let s = settings::load(&path)?;

    match args.command {
        MigrateSubcommand::Claude {
            file,
            namespace,
            classify,
            dry_run,
        } => {
            let opts = clio_core::migrate::MigrateOptions {
                namespace,
                classify,
                store: !dry_run,
            };
            let mut reader: Box<dyn io::Read> = if file == "-" {
                Box::new(io::stdin())
            } else {
                Box::new(std::fs::File::open(&file)?)
            };
            let result = clio_core::migrate::migrate_claude(&conn, &mut reader, &opts, &s)?;
            print_migration_result(&result, json, dry_run)?;
        }
        MigrateSubcommand::Chatgpt {
            file,
            namespace,
            classify,
            dry_run,
        } => {
            let opts = clio_core::migrate::MigrateOptions {
                namespace,
                classify,
                store: !dry_run,
            };
            let mut reader: Box<dyn io::Read> = if file == "-" {
                Box::new(io::stdin())
            } else {
                Box::new(std::fs::File::open(&file)?)
            };
            let result = clio_core::migrate::migrate_chatgpt(&conn, &mut reader, &opts, &s)?;
            print_migration_result(&result, json, dry_run)?;
        }
    }

    Ok(())
}

fn print_migration_result(
    result: &clio_core::migrate::MigrationResult,
    json: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        println!("{}", serde_json::to_string_pretty(result)?);
    } else {
        if dry_run {
            eprintln!("Dry run — no memories were stored.");
        }
        eprintln!(
            "Migration: {} imported, {} skipped, {} duplicates.",
            result.imported, result.skipped, result.duplicates
        );
        if !result.errors.is_empty() {
            eprintln!("Errors:");
            for err in &result.errors {
                eprintln!("  {err}");
            }
        }
        if !result.preview.is_empty() {
            eprintln!();
            eprintln!("Preview:");
            for entry in &result.preview {
                eprintln!(
                    "  [{}/{}] {} — {}",
                    entry.source,
                    entry.kind,
                    entry.title.as_deref().unwrap_or("(untitled)"),
                    entry.content_preview
                );
            }
        }
    }
    Ok(())
}

fn cmd_embed(db_path: Option<&str>, args: EmbedArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;

    match args.command {
        EmbedSubcommand::Status => {
            let unembedded = embeddings::count_unembedded(&conn)?;
            let total: u32 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
            let embedded = total.saturating_sub(unembedded);
            eprintln!("Embedding status:");
            eprintln!("  Total memories: {total}");
            eprintln!("  With embeddings: {embedded}");
            eprintln!("  Without embeddings: {unembedded}");

            let s = settings::load(&path)?;
            eprintln!("  Provider: {:?}", s.embeddings);
            eprintln!("  Auto-embed: {}", if s.auto_embed { "on" } else { "off" });
        }
        EmbedSubcommand::Backfill { batch_size } => {
            let s = settings::load(&path)?;
            let backend = embeddings::create_backend(&s.embeddings)?;

            let ids = embeddings::list_unembedded(&conn, batch_size)?;
            if ids.is_empty() {
                eprintln!("All memories already have embeddings.");
                return Ok(());
            }

            eprintln!("Embedding {} memories...", ids.len());
            let mut success = 0u32;
            let mut failed = 0u32;

            for id in &ids {
                match repository::get(&conn, id) {
                    Ok(memory) => {
                        match embeddings::embed_and_store(&conn, backend.as_ref(), &memory) {
                            Ok(()) => success += 1,
                            Err(e) => {
                                eprintln!("  Failed {id}: {e}");
                                failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  Could not load {id}: {e}");
                        failed += 1;
                    }
                }
            }

            eprintln!("Done: {success} embedded, {failed} failed.");
        }
    }

    Ok(())
}

fn cmd_settings(
    db_path: Option<&str>,
    json: bool,
    args: SettingsArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;

    match args.command {
        SettingsSubcommand::Show => {
            let s = settings::load(&path)?;
            if json {
                // Redact API keys in JSON output.
                let mut val = serde_json::to_value(&s)?;
                redact_api_keys(&mut val);
                println!("{}", serde_json::to_string_pretty(&val)?);
            } else {
                eprintln!(
                    "Settings (from {}):",
                    settings::settings_path(&path).display()
                );
                eprintln!(
                    "  Embedding provider: {}",
                    redact_embedding_display(&s.embeddings)
                );
                eprintln!("  Auto-embed: {}", if s.auto_embed { "on" } else { "off" });
                eprintln!(
                    "  Capture enabled: {}",
                    if s.capture.enabled { "on" } else { "off" }
                );
                eprintln!("  Capture model: {}", s.capture.model);
            }
        }
        SettingsSubcommand::UseLocal => {
            let mut s = settings::load(&path)?;
            s.embeddings = embeddings::EmbeddingConfig::Local {
                model: "all-MiniLM-L6-v2".into(),
            };
            settings::save(&path, &s)?;
            eprintln!("Embedding provider set to local (all-MiniLM-L6-v2).");
        }
        SettingsSubcommand::UseOpenai {
            api_key,
            model,
            base_url,
        } => {
            let mut s = settings::load(&path)?;
            s.embeddings = embeddings::EmbeddingConfig::OpenAi {
                api_key,
                model,
                base_url,
            };
            settings::save(&path, &s)?;
            eprintln!("Embedding provider set to OpenAI.");
        }
        SettingsSubcommand::Disable => {
            let mut s = settings::load(&path)?;
            s.embeddings = embeddings::EmbeddingConfig::Disabled;
            s.auto_embed = false;
            settings::save(&path, &s)?;
            eprintln!("Embeddings disabled.");
        }
        SettingsSubcommand::UseCapture {
            api_key,
            model,
            base_url,
        } => {
            let mut s = settings::load(&path)?;
            s.capture = settings::CaptureConfig {
                enabled: true,
                api_key: Some(api_key),
                model,
                base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".into()),
                review_threshold: s.capture.review_threshold,
            };
            settings::save(&path, &s)?;
            eprintln!("Capture pipeline enabled.");
        }
        SettingsSubcommand::DisableCapture => {
            let mut s = settings::load(&path)?;
            s.capture.enabled = false;
            settings::save(&path, &s)?;
            eprintln!("Capture pipeline disabled.");
        }
    }

    Ok(())
}

fn cmd_stats(
    db_path: Option<&str>,
    json: bool,
    args: StatsArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let memory_stats = stats::memory_stats(&conn, args.namespace.as_deref())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&memory_stats)?);
    } else {
        print_stats(&memory_stats);
    }

    Ok(())
}

fn cmd_activity(
    db_path: Option<&str>,
    json: bool,
    args: ActivityArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db(db_path)?;
    let entries = stats::recent_activity(&conn, args.namespace.as_deref(), args.limit)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        print_activity(&entries);
    }

    Ok(())
}

fn cmd_brief(
    db_path: Option<&str>,
    json: bool,
    args: BriefArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;

    let preset: ContextPreset = args.preset.parse()?;

    let namespace = match &args.namespace {
        Some(ns) => Some(ns.clone()),
        None => {
            let cwd = std::env::current_dir().ok();
            let stgs = settings::load(&path).ok().unwrap_or_default();
            if stgs.context.auto_detect {
                cwd.as_deref()
                    .and_then(context::detect_namespace)
                    .map(|ctx| ctx.namespace)
            } else {
                None
            }
        }
    };

    let stgs = settings::load(&path).ok().unwrap_or_default();

    let request = ContextRequest {
        namespace,
        preset,
        query: args.query,
        max_items: args.max_items,
        include_links: args.include_links,
        scoring: Some(stgs.scoring),
    };

    let brief = assembly::build_context(&conn, &request)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&brief)?);
    } else {
        print_brief(&brief);
    }

    Ok(())
}

fn print_brief(brief: &ContextBrief) {
    println!("# Context Brief: {}", brief.preset);
    println!(
        "Namespace: {}  |  Generated: {}",
        brief.namespace, brief.generated_at
    );
    println!("Total memories: {}", brief.total_memories_used);
    println!();

    for section in &brief.sections {
        println!("## {}", section.heading);
        println!();

        if section.items.is_empty() {
            println!("  (no memories)");
            println!();
            continue;
        }

        for mem in &section.items {
            let title = mem.title.as_deref().unwrap_or("(untitled)");
            let preview = mem.summary.as_deref().unwrap_or(&mem.content);
            let truncated = if preview.chars().count() > 120 {
                let s: String = preview.chars().take(120).collect();
                format!("{s}...")
            } else {
                preview.to_string()
            };
            println!("  - [{}] **{}** ({})", mem.kind, title, mem.id);
            println!("    {}", truncated);
        }

        println!();
    }
}

fn cmd_suggest_links(
    db_path: Option<&str>,
    json: bool,
    args: SuggestLinksArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_db_path(db_path)?;
    let conn = db::open(&path)?;
    let stgs = settings::load(&path)?;

    let backend = embeddings::create_backend(&stgs.embeddings)?;
    let suggestions = embeddings::suggest_links(
        &conn,
        &args.memory_id,
        backend.as_ref(),
        args.threshold,
        args.limit,
    )?;

    if json {
        let items: Vec<serde_json::Value> = suggestions
            .iter()
            .map(|(mem, sim)| {
                serde_json::json!({
                    "memory": mem,
                    "similarity": sim,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        print_suggestions(&suggestions);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// XML escape helper
// ---------------------------------------------------------------------------

/// Escape a string for safe inclusion in XML/plist `<string>` elements.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ---------------------------------------------------------------------------
// API key redaction helpers
// ---------------------------------------------------------------------------

/// Redact `api_key` fields in a JSON value tree (for safe display).
fn redact_api_keys(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key == "api_key" {
                    let should_redact = matches!(v, serde_json::Value::String(s) if !s.is_empty());
                    if should_redact {
                        *v = serde_json::Value::String("***".into());
                    }
                } else {
                    redact_api_keys(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                redact_api_keys(v);
            }
        }
        _ => {}
    }
}

/// Human-readable embedding config display with redacted API key.
fn redact_embedding_display(config: &embeddings::EmbeddingConfig) -> String {
    match config {
        embeddings::EmbeddingConfig::Local { model } => format!("Local ({model})"),
        embeddings::EmbeddingConfig::OpenAi { model, api_key, .. } => {
            let key_status = match api_key {
                Some(k) if !k.is_empty() => "***",
                _ => "not set",
            };
            format!("OpenAI ({model}, api_key: {key_status})")
        }
        embeddings::EmbeddingConfig::Disabled => "Disabled".into(),
    }
}

// ---------------------------------------------------------------------------
// Human-readable formatting
// ---------------------------------------------------------------------------

const BOX_WIDTH: usize = 50;

fn print_memory_card(m: &Memory) {
    let title_display = m.title.as_deref().unwrap_or("Untitled");
    let header = format!(" {} ", title_display);
    let pad = BOX_WIDTH.saturating_sub(header.len() + 2);

    // Top border with title
    println!(
        "\u{256d}\u{2500} {}{} \u{256e}",
        header,
        "\u{2500}".repeat(pad)
    );

    print_field("id", &m.id);
    print_field("namespace", &m.namespace);
    print_field("kind", &m.kind);

    if !m.tags.is_empty() {
        print_field("tags", &m.tags.join(", "));
    }

    print_field("importance", &m.importance.to_string());

    if let Some(conf) = m.confidence {
        print_field("confidence", &format!("{conf:.2}"));
    }

    if let Some(ref src) = m.source {
        print_field("source", src);
    }

    if let Some(ref src_ref) = m.source_ref {
        print_field("source_ref", src_ref);
    }

    print_field("created", &m.created_at);
    print_field("updated", &m.updated_at);

    if let Some(ref archived) = m.archived_at {
        print_field("archived", archived);
    }

    if let Some(ref vf) = m.valid_from {
        print_field("valid_from", vf);
    }

    if let Some(ref vu) = m.valid_until {
        print_field("valid_until", vu);
    }

    // Metadata (only if non-empty object)
    if let Some(obj) = m.metadata.as_object() {
        if !obj.is_empty() {
            print_field(
                "metadata",
                &serde_json::to_string(&m.metadata).unwrap_or_default(),
            );
        }
    }

    // Divider
    println!("\u{251c}{}\u{256f}", "\u{2500}".repeat(BOX_WIDTH));

    // Summary
    if let Some(ref summary) = m.summary {
        for line in summary.lines() {
            println!("\u{2502} {line}");
        }
        println!("\u{251c}\u{2500}\u{2500}\u{2500}");
    }

    // Content
    for line in m.content.lines() {
        println!("\u{2502} {line}");
    }

    // Bottom border
    println!("\u{2570}{}\u{256f}", "\u{2500}".repeat(BOX_WIDTH));
}

fn print_review_item(item: &review::ReviewItem) {
    let title_display = item.suggested_title.as_deref().unwrap_or("Untitled");
    let header = format!(" [review] {} ", title_display);
    let pad = BOX_WIDTH.saturating_sub(header.len() + 2);

    println!(
        "\u{256d}\u{2500} {}{} \u{256e}",
        header,
        "\u{2500}".repeat(pad)
    );

    print_field("id", &item.id);
    print_field("status", &item.status);
    print_field("namespace", &item.suggested_namespace);
    print_field("kind", &item.suggested_kind);

    if !item.suggested_tags.is_empty() {
        print_field("tags", &item.suggested_tags.join(", "));
    }

    print_field("importance", &item.suggested_importance.to_string());

    if let Some(conf) = item.suggested_confidence {
        print_field("confidence", &format!("{conf:.2}"));
    }

    if let Some(ref route) = item.source_route {
        print_field("source", route);
    }

    print_field("created", &item.created_at);

    if let Some(ref reviewed) = item.reviewed_at {
        print_field("reviewed", reviewed);
    }

    // Divider
    println!("\u{251c}{}\u{256f}", "\u{2500}".repeat(BOX_WIDTH));

    // Summary
    if let Some(ref summary) = item.suggested_summary {
        for line in summary.lines() {
            println!("\u{2502} {line}");
        }
        println!("\u{251c}\u{2500}\u{2500}\u{2500}");
    }

    // Content (truncated)
    let preview = if item.content.chars().count() > 300 {
        let s: String = item.content.chars().take(300).collect();
        format!("{s}...")
    } else {
        item.content.clone()
    };
    for line in preview.lines() {
        println!("\u{2502} {line}");
    }

    // Bottom border
    println!("\u{2570}{}\u{256f}", "\u{2500}".repeat(BOX_WIDTH));
}

fn print_field(label: &str, value: &str) {
    println!("\u{2502} {:<12}{}", format!("{label}:"), value);
}

fn print_recall_result(result: &RecallResult) {
    if result.items.is_empty() {
        println!("No memories found.");
        return;
    }

    println!(
        "Showing {}-{} of {} memories",
        result.offset + 1,
        result.offset + result.count,
        result.total
    );
    println!();

    for item in &result.items {
        let m = &item.memory;
        let title = m.title.as_deref().unwrap_or("(untitled)");
        let ns = &m.namespace;
        let kind = &m.kind;
        let archived_marker = if m.archived_at.is_some() {
            " [archived]"
        } else {
            ""
        };

        print!("  {}", &m.id[..std::cmp::min(m.id.len(), 8)]);
        print!("  {ns}/{kind}");
        print!("  {title}");
        print!("{archived_marker}");

        if let Some(rank) = item.rank {
            print!("  (rank: {rank:.4})");
        }

        println!();

        // Show a truncated summary or content preview
        if let Some(ref summary) = m.summary {
            let preview = truncate(summary, 80);
            println!("           {preview}");
        } else {
            let preview = truncate(&m.content, 80);
            println!("           {preview}");
        }
    }

    if result.total > result.offset + result.count {
        println!();
        println!("Use --offset {} to see more.", result.offset + result.count);
    }
}

fn print_link(link: &MemoryLink) {
    println!("Link created:");
    println!(
        "  {} --[{}]--> {}",
        &link.from_memory_id[..std::cmp::min(link.from_memory_id.len(), 8)],
        link.relationship,
        &link.to_memory_id[..std::cmp::min(link.to_memory_id.len(), 8)]
    );
    println!("  created: {}", link.created_at);
}

fn truncate(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or("");
    if first_line.chars().count() <= max {
        first_line.to_string()
    } else {
        let s: String = first_line.chars().take(max.saturating_sub(3)).collect();
        format!("{s}...")
    }
}

fn print_stats(st: &MemoryStats) {
    println!("Memory Statistics");
    println!("=================");
    println!("  Total memories:     {}", st.total_memories);
    println!("  Active:             {}", st.active_memories);
    println!("  Archived:           {}", st.archived_memories);
    println!("  Total embeddings:   {}", st.total_embeddings);
    println!("  Embedding coverage: {:.1}%", st.embedding_coverage);
    println!("  Total links:        {}", st.total_links);
    println!("  Link density:       {:.2} links/memory", st.link_density);

    if !st.by_namespace.is_empty() {
        println!();
        println!("By Namespace:");
        for (ns, count) in &st.by_namespace {
            println!("  {ns:<30} {count}");
        }
    }

    if !st.by_kind.is_empty() {
        println!();
        println!("By Kind:");
        for (kind, count) in &st.by_kind {
            println!("  {kind:<30} {count}");
        }
    }

    if !st.top_tags.is_empty() {
        println!();
        println!("Top Tags:");
        for (tag, count) in &st.top_tags {
            println!("  {tag:<30} {count}");
        }
    }

    if !st.by_week.is_empty() {
        println!();
        println!("Weekly Timeline:");
        for (week, count) in &st.by_week {
            println!("  {week:<20} {count}");
        }
    }
}

fn print_activity(entries: &[RecentEntry]) {
    if entries.is_empty() {
        println!("No recent activity.");
        return;
    }

    println!("Recent Activity");
    println!("===============");
    println!();

    for entry in entries {
        let title = entry.title.as_deref().unwrap_or("(untitled)");
        let id_short = &entry.memory_id[..std::cmp::min(entry.memory_id.len(), 8)];
        println!(
            "  {:<10} {id_short}  {}/{} - {title}",
            entry.action, entry.namespace, entry.kind
        );
        println!("             {}", entry.timestamp);
    }
}

fn print_suggestions(suggestions: &[(Memory, f64)]) {
    if suggestions.is_empty() {
        println!("No link suggestions found above the threshold.");
        return;
    }

    println!("Suggested Links");
    println!("===============");
    println!();

    for (mem, similarity) in suggestions {
        let title = mem.title.as_deref().unwrap_or("(untitled)");
        let id_short = &mem.id[..std::cmp::min(mem.id.len(), 8)];
        println!(
            "  {id_short}  {}/{} - {title}  (similarity: {similarity:.4})",
            mem.namespace, mem.kind
        );
    }
}

// ---------------------------------------------------------------------------
// serve — start the MCP server
// ---------------------------------------------------------------------------

fn cmd_serve(explicit_db: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let mcp_binary = find_mcp_binary()?;

    // Verify the database is initialised before handing off.
    let db_path = resolve_db_path(explicit_db)?;
    let conn = db::open(&db_path)?;
    drop(conn);

    eprintln!("Starting Clio MCP server (stdio transport)...");
    eprintln!("Binary:   {}", mcp_binary.display());
    eprintln!("Database: {}", db_path.display());
    eprintln!("Press Ctrl+C to stop.\n");

    let mut cmd = process::Command::new(&mcp_binary);
    cmd.env("CLIO_DB_PATH", &db_path);
    cmd.stdin(process::Stdio::inherit());
    cmd.stdout(process::Stdio::inherit());
    cmd.stderr(process::Stdio::inherit());

    let status = cmd.status()?;
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn find_mcp_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Check adjacent to the current executable (installed together).
    if let Ok(exe) = std::env::current_exe() {
        let adjacent = exe.with_file_name("clio-mcp");
        if adjacent.exists() {
            return Ok(adjacent);
        }
    }

    // Check PATH.
    if let Ok(output) = process::Command::new("which").arg("clio-mcp").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    Err("Could not find clio-mcp binary. Ensure it is installed alongside clio or available in PATH.".into())
}

// ---------------------------------------------------------------------------
// daemon — manage the Clio background daemon
// ---------------------------------------------------------------------------

fn cmd_daemon(
    explicit_db: Option<&str>,
    json: bool,
    command: DaemonCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        DaemonCommand::Run => cmd_daemon_run(explicit_db),
        DaemonCommand::Start => cmd_daemon_start(explicit_db),
        DaemonCommand::Stop => cmd_daemon_stop(explicit_db, json),
        DaemonCommand::Restart => {
            // Stop (ignore errors — it may not be running).
            let _ = cmd_daemon_stop(explicit_db, false);
            cmd_daemon_start(explicit_db)
        }
        DaemonCommand::Status => cmd_daemon_status(explicit_db, json),
        DaemonCommand::Logs { lines } => cmd_daemon_logs(explicit_db, lines),
        DaemonCommand::Install => cmd_daemon_install(explicit_db),
        DaemonCommand::Uninstall => cmd_daemon_uninstall(),
        DaemonCommand::Doctor => cmd_daemon_doctor(explicit_db, json),
    }
}

fn find_daemon_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Check adjacent to the current executable (installed together).
    if let Ok(exe) = std::env::current_exe() {
        let adjacent = exe.with_file_name("clio-daemon");
        if adjacent.exists() {
            return Ok(adjacent);
        }
    }

    // Check PATH.
    if let Ok(output) = process::Command::new("which").arg("clio-daemon").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    Err("Could not find clio-daemon binary. Ensure it is installed alongside clio or available in PATH.".into())
}

/// Connect to the daemon control socket, send a command, and return the response.
fn send_daemon_command(
    socket_path: &Path,
    command: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let stream = UnixStream::connect(socket_path).map_err(|e| {
        format!(
            "could not connect to daemon socket at {}: {e}",
            socket_path.display()
        )
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut writer = stream.try_clone()?;
    let payload = serde_json::to_string(&serde_json::json!({"command": command}))?;
    writer.write_all(payload.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;

    Ok(response)
}

/// Resolve the socket path from settings or fall back to the platform default.
fn resolve_socket_path(explicit_db: Option<&str>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let db_path = resolve_db_path(explicit_db)?;
    let s = settings::load(&db_path)?;

    if let Some(ref p) = s.daemon.socket_path {
        return Ok(p.clone());
    }

    Ok(daemon::default_socket_path()?)
}

/// Resolve the log directory from settings or fall back to the platform default.
fn resolve_log_dir(explicit_db: Option<&str>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let db_path = resolve_db_path(explicit_db)?;
    let s = settings::load(&db_path)?;

    if let Some(ref p) = s.daemon.log_dir {
        return Ok(p.clone());
    }

    Ok(daemon::default_log_dir()?)
}

fn cmd_daemon_run(explicit_db: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let daemon_bin = find_daemon_binary()?;
    let db_path = resolve_db_path(explicit_db)?;

    eprintln!("Starting Clio daemon (foreground)...");
    eprintln!("Binary:   {}", daemon_bin.display());
    eprintln!("Database: {}", db_path.display());

    let mut cmd = process::Command::new(&daemon_bin);
    cmd.arg("--db-path").arg(&db_path);
    cmd.stdin(process::Stdio::inherit());
    cmd.stdout(process::Stdio::inherit());
    cmd.stderr(process::Stdio::inherit());

    let status = cmd.status()?;
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn cmd_daemon_start(explicit_db: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let daemon_bin = find_daemon_binary()?;
    let db_path = resolve_db_path(explicit_db)?;

    // Check if already running via the PID file.
    if let Ok(pid_path) = daemon::default_pid_path() {
        let pid_file = daemon::PidFile::new(pid_path);
        if pid_file.is_running() {
            if let Ok(Some(pid)) = pid_file.read() {
                eprintln!("Daemon is already running (PID {pid}).");
                return Ok(());
            }
        }
    }

    // Resolve log directory from already-resolved db_path (avoids re-resolving).
    let s = settings::load(&db_path)?;
    let log_dir = s
        .daemon
        .log_dir
        .clone()
        .or_else(|| daemon::default_log_dir().ok())
        .ok_or("could not determine log directory")?;
    std::fs::create_dir_all(&log_dir)?;

    let stdout_path = log_dir.join("clio-daemon.stdout.log");
    let stderr_path = log_dir.join("clio-daemon.stderr.log");

    let stdout_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stdout_path)?;
    let stderr_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stderr_path)?;

    let child = process::Command::new(&daemon_bin)
        .arg("--db-path")
        .arg(&db_path)
        .stdin(process::Stdio::null())
        .stdout(stdout_file)
        .stderr(stderr_file)
        .spawn()?;

    eprintln!("Daemon started (PID {}).", child.id());
    eprintln!("Logs: {}", log_dir.display());

    Ok(())
}

fn cmd_daemon_stop(
    explicit_db: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = resolve_socket_path(explicit_db)?;

    match send_daemon_command(&socket_path, "stop") {
        Ok(response) => {
            if json {
                // Pass through the daemon's JSON response.
                print!("{response}");
            } else {
                let v: serde_json::Value = serde_json::from_str(response.trim())
                    .unwrap_or_else(|_| serde_json::json!({"raw": response.trim()}));
                if v.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                    eprintln!("Daemon is stopping.");
                } else if let Some(err) = v.get("error") {
                    eprintln!("Daemon returned an error: {err}");
                    process::exit(1);
                } else {
                    eprintln!("Daemon responded: {}", response.trim());
                }
            }
        }
        Err(e) => {
            // Socket not available — check PID file for a stale process.
            if let Ok(pid_path) = daemon::default_pid_path() {
                let pid_file = daemon::PidFile::new(pid_path);
                if pid_file.is_running() {
                    eprintln!(
                        "Could not connect to daemon socket, but process appears to be running."
                    );
                    eprintln!("Socket error: {e}");
                    process::exit(1);
                }
            }
            eprintln!("Daemon is not running.");
        }
    }

    Ok(())
}

fn cmd_daemon_status(
    explicit_db: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = resolve_socket_path(explicit_db)?;

    match send_daemon_command(&socket_path, "status") {
        Ok(response) => {
            if json {
                print!("{response}");
                return Ok(());
            }

            let status: DaemonStatus = serde_json::from_str(response.trim()).map_err(|e| {
                format!("failed to parse daemon status: {e}\nRaw response: {response}")
            })?;

            eprintln!("Clio daemon status:");
            if let Some(pid) = status.pid {
                eprintln!("  PID:            {pid}");
            }
            if let Some(secs) = status.uptime_secs {
                eprintln!("  Uptime:         {}", format_uptime(secs));
            }
            eprintln!("  Database:       {}", status.db_path);
            if !status.enabled_routes.is_empty() {
                eprintln!("  Routes:         {}", status.enabled_routes.join(", "));
            }
            eprintln!();
            eprintln!("  Health:");
            print_health_line("Database", &status.health.database);
            print_health_line("Embeddings", &status.health.embeddings);
            print_health_line("Capture", &status.health.capture);
        }
        Err(_) => {
            // Daemon not reachable — check PID file.
            if let Ok(pid_path) = daemon::default_pid_path() {
                let pid_file = daemon::PidFile::new(pid_path);
                if pid_file.is_running() {
                    if let Ok(Some(pid)) = pid_file.read() {
                        eprintln!(
                            "Daemon process is running (PID {pid}) but the control socket is not responding."
                        );
                        process::exit(1);
                    }
                }
            }

            if json {
                println!("{{\"running\":false}}");
            } else {
                eprintln!("Daemon is not running.");
            }
        }
    }

    Ok(())
}

fn cmd_daemon_logs(
    explicit_db: Option<&str>,
    lines: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let log_dir = resolve_log_dir(explicit_db)?;

    // Look for the most recent daily log file matching the daemon pattern.
    let mut log_files: Vec<PathBuf> = Vec::new();
    if log_dir.is_dir() {
        for entry in std::fs::read_dir(&log_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("clio-daemon.log") {
                log_files.push(entry.path());
            }
        }
    }

    if log_files.is_empty() {
        eprintln!("No daemon log files found in {}", log_dir.display());
        return Ok(());
    }

    // Sort by name so the most recent rotated log comes last.
    log_files.sort();
    let Some(latest) = log_files.last() else {
        // Unreachable: is_empty() check above guarantees at least one entry.
        eprintln!("No daemon log files found in {}", log_dir.display());
        return Ok(());
    };

    let content = std::fs::read_to_string(latest)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);

    eprintln!("-- {} (last {} lines) --", latest.display(), lines);
    for line in &all_lines[start..] {
        println!("{line}");
    }

    Ok(())
}

fn cmd_daemon_install(explicit_db: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let daemon_bin = find_daemon_binary()?;
    let db_path = resolve_db_path(explicit_db)?;
    let s = settings::load(&db_path)?;
    let log_dir = s
        .daemon
        .log_dir
        .clone()
        .or_else(|| daemon::default_log_dir().ok())
        .ok_or("could not determine log directory")?;

    let home = std::env::var("HOME").map_err(|_| "could not determine home directory")?;
    let plist_dir = PathBuf::from(&home).join("Library").join("LaunchAgents");
    std::fs::create_dir_all(&plist_dir)?;
    let plist_path = plist_dir.join("com.clio.daemon.plist");

    let stdout_log = log_dir.join("clio-daemon.stdout.log");
    let stderr_log = log_dir.join("clio-daemon.stderr.log");
    std::fs::create_dir_all(&log_dir)?;

    // XML-escape interpolated paths to prevent malformed XML from paths
    // containing characters like <, >, &, or quotes.
    let daemon_bin_esc = xml_escape(&daemon_bin.display().to_string());
    let db_path_esc = xml_escape(&db_path.display().to_string());
    let stdout_log_esc = xml_escape(&stdout_log.display().to_string());
    let stderr_log_esc = xml_escape(&stderr_log.display().to_string());

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.clio.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{daemon_bin_esc}</string>
        <string>--db-path</string>
        <string>{db_path_esc}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout_log_esc}</string>
    <key>StandardErrorPath</key>
    <string>{stderr_log_esc}</string>
</dict>
</plist>
"#,
    );

    std::fs::write(&plist_path, &plist_content)?;
    eprintln!("Wrote LaunchAgent plist to {}", plist_path.display());

    let status = process::Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist_path)
        .status()?;

    if status.success() {
        eprintln!("LaunchAgent loaded. The daemon will start automatically on login.");
    } else {
        eprintln!("launchctl load failed (exit code {:?}).", status.code());
        eprintln!(
            "You may need to load it manually: launchctl load -w {}",
            plist_path.display()
        );
        process::exit(1);
    }

    Ok(())
}

fn cmd_daemon_uninstall() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").map_err(|_| "could not determine home directory")?;
    let plist_path = PathBuf::from(&home)
        .join("Library")
        .join("LaunchAgents")
        .join("com.clio.daemon.plist");

    if !plist_path.exists() {
        eprintln!("LaunchAgent plist not found at {}", plist_path.display());
        eprintln!("Nothing to uninstall.");
        return Ok(());
    }

    let status = process::Command::new("launchctl")
        .args(["unload", "-w"])
        .arg(&plist_path)
        .status()?;

    if !status.success() {
        eprintln!("launchctl unload returned exit code {:?}.", status.code());
    }

    std::fs::remove_file(&plist_path)?;
    eprintln!("Removed LaunchAgent plist from {}", plist_path.display());
    eprintln!("The daemon will no longer start automatically on login.");

    Ok(())
}

fn cmd_daemon_doctor(
    explicit_db: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = resolve_db_path(explicit_db)?;
    let s = settings::load(&db_path)?;
    let health = daemon::run_health_checks(&db_path, &s);

    if json {
        println!("{}", serde_json::to_string_pretty(&health)?);
        return Ok(());
    }

    eprintln!("Clio doctor");
    print_health_line("Database", &health.database);
    print_health_line("Embeddings", &health.embeddings);
    print_health_line("Capture", &health.capture);

    // Exit non-zero if any check is unhealthy.
    if health.database.status == HealthStatus::Unhealthy
        || health.embeddings.status == HealthStatus::Unhealthy
        || health.capture.status == HealthStatus::Unhealthy
    {
        process::exit(1);
    }

    Ok(())
}

fn print_health_line(label: &str, check: &daemon::HealthCheck) {
    let marker = match check.status {
        HealthStatus::Healthy => "[OK]",
        HealthStatus::Degraded => "[!!]",
        HealthStatus::Unhealthy => "[FAIL]",
        HealthStatus::Unconfigured => "[--]",
    };
    eprintln!("  {label:<14}{marker} {}", check.message);
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

// ---------------------------------------------------------------------------
// setup — generate MCP client configuration
// ---------------------------------------------------------------------------

fn cmd_setup(
    explicit_db: Option<&str>,
    json_output: bool,
    args: SetupArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mcp_binary = find_mcp_binary()?;
    let db_path = resolve_db_path(explicit_db)?;
    let binary_str = mcp_binary.display().to_string();
    let db_str = db_path.display().to_string();
    let dry_run = args.dry_run;

    // Ensure DB and inbox exist (idempotent — safe to re-run).
    let _conn = clio_core::db::open(&db_path)?;
    let _inbox = clio_core::config::ensure_inbox_dir(&db_path)?;

    let home = std::env::var("HOME")
        .map_err(|_| "HOME environment variable is not set; cannot locate config directories")?;

    // The clio server entry for standard mcpServers JSON.
    let clio_server = serde_json::json!({
        "command": binary_str,
        "env": {
            "CLIO_DB_PATH": db_str
        }
    });

    // Each client has a config file path and a strategy for merging.
    match args.client {
        SetupClient::Generic => {
            // Generic just prints — no auto-install.
            let config = serde_json::json!({ "mcpServers": { "clio": clio_server } });
            if json_output {
                println!("{}", serde_json::to_string_pretty(&config)?);
            } else {
                println!("Add the following to your MCP client configuration.\n");
                println!("{}", serde_json::to_string_pretty(&config)?);
                println!();
                println!("Binary:   {binary_str}");
                println!("Database: {db_str}");
            }
        }

        SetupClient::Codex => {
            // Codex uses TOML — append to ~/.codex/config.toml.
            let config_path = PathBuf::from(&home).join(".codex").join("config.toml");
            let toml_block = format!(
                "\n[mcp_servers.clio]\ncommand = \"{binary_str}\"\n\n\
                 [mcp_servers.clio.env]\nCLIO_DB_PATH = \"{db_str}\"\n"
            );

            if dry_run || json_output {
                if json_output {
                    let config = serde_json::json!({ "mcpServers": { "clio": clio_server } });
                    println!("{}", serde_json::to_string_pretty(&config)?);
                } else {
                    println!("Would write to: {}\n", config_path.display());
                    print!("{toml_block}");
                }
            } else {
                // Check if clio is already configured.
                let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
                if existing.contains("[mcp_servers.clio]") {
                    println!("Clio is already configured in {}", config_path.display());
                    println!(
                        "Remove the existing [mcp_servers.clio] section first to reconfigure."
                    );
                    return Ok(());
                }
                if let Some(parent) = config_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&config_path)?;
                std::io::Write::write_all(&mut file, toml_block.as_bytes())?;
                println!("Installed Clio MCP server into {}", config_path.display());
                println!("Binary:   {binary_str}");
                println!("Database: {db_str}");
            }
        }

        SetupClient::Opencode => {
            // OpenCode uses a different JSON shape: mcp.clio with type/command array/environment.
            let config_path = PathBuf::from(&home)
                .join(".config")
                .join("opencode")
                .join("opencode.json");
            let clio_entry = serde_json::json!({
                "type": "local",
                "command": [binary_str],
                "environment": {
                    "CLIO_DB_PATH": db_str
                }
            });
            setup_json_merge(&SetupMergeParams {
                config_path: &config_path,
                root_key: "mcp",
                server_name: "clio",
                server_value: &clio_entry,
                dry_run,
                json_output,
                binary_str: &binary_str,
                db_str: &db_str,
            })?;
        }

        // All remaining clients use standard mcpServers JSON.
        client => {
            let config_path = match client {
                SetupClient::ClaudeCode => PathBuf::from(&home).join(".claude.json"),
                SetupClient::Cursor => PathBuf::from(&home).join(".cursor").join("mcp.json"),
                SetupClient::Windsurf => PathBuf::from(&home).join(".windsurf").join("mcp.json"),
                SetupClient::Kilo => PathBuf::from(&home).join(".kilocode").join("mcp.json"),
                SetupClient::Kimi => PathBuf::from(&home).join(".kimi").join("mcp.json"),
                SetupClient::Copilot => PathBuf::from(&home)
                    .join(".copilot")
                    .join("mcp-config.json"),
                SetupClient::Gemini => PathBuf::from(&home).join(".gemini").join("settings.json"),
                _ => return Err("unhandled setup client variant".into()),
            };
            setup_json_merge(&SetupMergeParams {
                config_path: &config_path,
                root_key: "mcpServers",
                server_name: "clio",
                server_value: &clio_server,
                dry_run,
                json_output,
                binary_str: &binary_str,
                db_str: &db_str,
            })?;
        }
    }

    Ok(())
}

/// Parameters for merging a server entry into a JSON config file.
struct SetupMergeParams<'a> {
    config_path: &'a Path,
    root_key: &'a str,
    server_name: &'a str,
    server_value: &'a serde_json::Value,
    dry_run: bool,
    json_output: bool,
    binary_str: &'a str,
    db_str: &'a str,
}

/// Merge a server entry into a JSON config file under the given top-level key.
///
/// Reads the existing file (or starts with `{}`), inserts `server_value` at
/// `root_key.server_name`, and writes it back. Preserves all other keys.
fn setup_json_merge(p: &SetupMergeParams<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = p.config_path;
    let root_key = p.root_key;
    let server_name = p.server_name;
    let server_value = p.server_value;

    // Read existing config or start fresh.
    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(config_path)?;
        serde_json::from_str(&content).map_err(|e| {
            format!(
                "Could not parse {}: {e}\nFix the file or use --json to get the config snippet.",
                config_path.display()
            )
        })?
    } else {
        serde_json::json!({})
    };

    // Check if clio is already configured.
    if config
        .get(root_key)
        .and_then(|v: &serde_json::Value| v.get(server_name))
        .is_some()
        && !p.dry_run
        && !p.json_output
    {
        println!("Clio is already configured in {}", config_path.display());
        println!(
            "To reconfigure, remove the \"{}\" entry from \"{}\" first.",
            server_name, root_key
        );
        return Ok(());
    }

    // Merge the server entry.
    if config.get(root_key).is_none() {
        config[root_key] = serde_json::json!({});
    }
    config[root_key][server_name] = server_value.clone();

    if p.dry_run || p.json_output {
        if p.json_output {
            // Just print the snippet, not the whole file.
            let snippet = serde_json::json!({ (root_key): { (server_name): server_value } });
            println!("{}", serde_json::to_string_pretty(&snippet)?);
        } else {
            println!("Would write to: {}\n", config_path.display());
            let snippet = serde_json::json!({ (root_key): { (server_name): server_value } });
            println!("{}", serde_json::to_string_pretty(&snippet)?);
        }
    } else {
        // Ensure parent directory exists.
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&config)?;
        std::fs::write(config_path, format!("{content}\n"))?;
        println!("Installed Clio MCP server into {}", config_path.display());
        println!("Binary:   {}", p.binary_str);
        println!("Database: {}", p.db_str);
    }

    Ok(())
}
