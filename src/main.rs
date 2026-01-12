use std::path::PathBuf;
use std::process::{Command, ExitCode};
use clap::{Parser, Subcommand};
use anyhow::Result;

use scopetest::config::Config;
use scopetest::builder::GraphBuilder;
use scopetest::cache::CacheManager;
use scopetest::git::GitChangeDetector;
use scopetest::affected::AffectedTestFinder;
use scopetest::output::{OutputFormat, OutputFormatter};

#[derive(Parser)]
#[command(name = "scopetest")]
#[command(about = "Smart test selector - run only tests affected by code changes")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Find tests affected by changes
    Affected {
        /// Git reference to compare against (branch, commit, tag)
        #[arg(short, long)]
        base: Option<String>,

        /// Output format: paths, list, json (aliases: jest, vitest)
        #[arg(short, long, default_value = "paths")]
        format: String,

        /// Output affected source files instead of tests
        #[arg(long)]
        sources: bool,

        /// Disable cache
        #[arg(long)]
        no_cache: bool,

        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,

        /// Execute command with {} replaced by affected files
        #[arg(short = 'x', long)]
        exec: Option<String>,
    },

    /// Build or rebuild the dependency graph
    Build {
        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Affected { base, format, sources, no_cache, root, exec } => {
            run_affected(base, format, sources, no_cache, root, exec)
        }
        Commands::Build { root } => {
            run_build(root)
        }
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn get_root(root: Option<PathBuf>) -> PathBuf {
    root.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn run_affected(
    base: Option<String>,
    format: String,
    sources: bool,
    no_cache: bool,
    root: Option<PathBuf>,
    exec: Option<String>,
) -> Result<ExitCode> {
    let root = get_root(root);
    let config = Config::load(&root)?;
    let cache = CacheManager::new(&root);

    // Load or build graph
    let graph = if !no_cache && config.cache_enabled {
        match cache.load() {
            Ok(Some(g)) => g,
            _ => {
                let builder = GraphBuilder::new(root.clone(), config.clone());
                let g = builder.build()?;
                let _ = cache.save(&g);
                g
            }
        }
    } else {
        let builder = GraphBuilder::new(root.clone(), config.clone());
        builder.build()?
    };

    // Detect changes
    let git = GitChangeDetector::new(root.clone())?;
    let base_ref = base.unwrap_or_else(|| git.get_default_base());
    let changes = git.detect_changes(&base_ref)?;

    // Find affected
    let finder = AffectedTestFinder::new(&graph);
    let result = finder.find_affected(&changes);
    let (total_tests, total_sources) = finder.get_totals();

    // Choose files based on --sources flag
    let files = if sources { &result.sources } else { &result.tests };

    // Format output
    let output_format: OutputFormat = format.parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    let output = match output_format {
        OutputFormat::Paths => OutputFormatter::format_paths(files),
        OutputFormat::Json => OutputFormatter::format_json(
            &result.tests,
            &result.sources,
            total_tests,
            total_sources,
        ),
        OutputFormat::List => OutputFormatter::format_list(files),
    };

    // Execute command or print output
    if let Some(cmd_template) = exec {
        if files.is_empty() {
            eprintln!("No affected files found.");
            return Ok(ExitCode::SUCCESS);
        }

        let files_str = OutputFormatter::format_paths(files);
        let cmd_str = cmd_template.replace("{}", &files_str);
        
        eprintln!("Running: {}", cmd_str);
        
        let status = Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .current_dir(&root)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if !status.success() {
            return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
        }
    } else {
        println!("{}", output);
    }

    Ok(ExitCode::SUCCESS)
}

fn run_build(root: Option<PathBuf>) -> Result<ExitCode> {
    let root = get_root(root);
    let config = Config::load(&root)?;
    let cache = CacheManager::new(&root);

    eprintln!("Building dependency graph...");
    
    let builder = GraphBuilder::new(root, config);
    let graph = builder.build()?;
    
    eprintln!("Found {} files with {} dependencies", graph.file_count(), graph.edge_count());
    
    cache.save(&graph)?;
    eprintln!("Cache saved.");

    Ok(ExitCode::SUCCESS)
}
