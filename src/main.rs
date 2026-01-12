use std::path::PathBuf;
use std::process::ExitCode;
use clap::{Parser, Subcommand};
use anyhow::Result;

use scopetest::config::Config;
use scopetest::builder::GraphBuilder;
use scopetest::cache::CacheManager;
use scopetest::git::GitChangeDetector;
use scopetest::affected::AffectedTestFinder;
use scopetest::output::{OutputFormat, OutputFormatter, CoverageThreshold};

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

        /// Output format: jest, json, list
        #[arg(short, long, default_value = "jest")]
        format: String,

        /// Disable cache
        #[arg(long)]
        no_cache: bool,

        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,
    },

    /// Build or rebuild the dependency graph
    Build {
        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,
    },

    /// Output coverage scope configuration
    Coverage {
        /// Git reference to compare against
        #[arg(short, long)]
        base: Option<String>,

        /// Coverage threshold percentage
        #[arg(short, long, default_value = "80")]
        threshold: u8,

        /// Output threshold config instead of file list
        #[arg(long)]
        threshold_config: bool,

        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Affected { base, format, no_cache, root } => {
            run_affected(base, format, no_cache, root)
        }
        Commands::Build { root } => {
            run_build(root)
        }
        Commands::Coverage { base, threshold, threshold_config, root } => {
            run_coverage(base, threshold, threshold_config, root)
        }
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
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
    no_cache: bool,
    root: Option<PathBuf>,
) -> Result<()> {
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
    let git = GitChangeDetector::new(root)?;
    let base_ref = base.unwrap_or_else(|| git.get_default_base());
    let changes = git.detect_changes(&base_ref)?;

    // Find affected tests
    let finder = AffectedTestFinder::new(&graph);
    let result = finder.find_affected(&changes);
    let (total_tests, total_sources) = finder.get_totals();

    // Format output
    let output_format: OutputFormat = format.parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    let output = match output_format {
        OutputFormat::Jest => OutputFormatter::format_jest_pattern(&result.tests),
        OutputFormat::Json => OutputFormatter::format_json(
            &result.tests,
            &result.sources,
            total_tests,
            total_sources,
        ),
        OutputFormat::List => OutputFormatter::format_list(&result.tests),
        OutputFormat::Paths => OutputFormatter::format_paths(&result.tests),
        OutputFormat::Coverage => OutputFormatter::format_coverage_from(&result.sources),
    };

    println!("{}", output);
    Ok(())
}

fn run_build(root: Option<PathBuf>) -> Result<()> {
    let root = get_root(root);
    let config = Config::load(&root)?;
    let cache = CacheManager::new(&root);

    eprintln!("Building dependency graph...");
    
    let builder = GraphBuilder::new(root, config);
    let graph = builder.build()?;
    
    eprintln!("Found {} files with {} dependencies", graph.file_count(), graph.edge_count());
    
    cache.save(&graph)?;
    eprintln!("Cache saved.");

    Ok(())
}

fn run_coverage(
    base: Option<String>,
    threshold: u8,
    threshold_config: bool,
    root: Option<PathBuf>,
) -> Result<()> {
    let root = get_root(root);
    let config = Config::load(&root)?;
    let cache = CacheManager::new(&root);

    // Load or build graph
    let graph = match cache.load() {
        Ok(Some(g)) => g,
        _ => {
            let builder = GraphBuilder::new(root.clone(), config.clone());
            let g = builder.build()?;
            let _ = cache.save(&g);
            g
        }
    };

    // Detect changes
    let git = GitChangeDetector::new(root)?;
    let base_ref = base.unwrap_or_else(|| git.get_default_base());
    let changes = git.detect_changes(&base_ref)?;

    // Find affected
    let finder = AffectedTestFinder::new(&graph);
    let result = finder.find_affected(&changes);

    // Output coverage config
    let output = if threshold_config {
        OutputFormatter::format_coverage_threshold(
            &result.sources,
            CoverageThreshold {
                branches: threshold,
                lines: threshold,
                functions: threshold,
                statements: threshold,
            },
        )
    } else {
        OutputFormatter::format_coverage_from(&result.sources)
    };

    println!("{}", output);
    Ok(())
}
