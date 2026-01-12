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

        /// Find changes since this commit (commit..HEAD range)
        #[arg(long, conflicts_with = "base")]
        since: Option<String>,

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

        /// Stop on first test failure (only with --exec)
        #[arg(long)]
        fail_fast: bool,

        /// If affected tests exceed this threshold, use all tests instead
        #[arg(long)]
        threshold: Option<usize>,
    },

    /// Explain why a test is affected by changes
    Why {
        /// The test file to explain
        test: PathBuf,

        /// Git reference to compare against (branch, commit, tag)
        #[arg(short, long)]
        base: Option<String>,

        /// Find changes since this commit (commit..HEAD range)
        #[arg(long, conflicts_with = "base")]
        since: Option<String>,

        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,

        /// Disable cache
        #[arg(long)]
        no_cache: bool,

        /// Show all paths, not just the shortest
        #[arg(long)]
        all: bool,
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
        Commands::Affected { 
            base, since, format, sources, no_cache, root, exec, fail_fast, threshold 
        } => {
            run_affected(base, since, format, sources, no_cache, root, exec, fail_fast, threshold)
        }
        Commands::Why { test, base, since, root, no_cache, all } => {
            run_why(test, base, since, root, no_cache, all)
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

fn load_graph(root: &PathBuf, config: &Config, cache: &CacheManager, no_cache: bool) -> Result<scopetest::DependencyGraph> {
    if !no_cache && config.cache_enabled {
        match cache.load() {
            Ok(Some(g)) => Ok(g),
            _ => {
                let builder = GraphBuilder::new(root.clone(), config.clone());
                let g = builder.build()?;
                let _ = cache.save(&g);
                Ok(g)
            }
        }
    } else {
        let builder = GraphBuilder::new(root.clone(), config.clone());
        Ok(builder.build()?)
    }
}

fn run_affected(
    base: Option<String>,
    since: Option<String>,
    format: String,
    sources: bool,
    no_cache: bool,
    root: Option<PathBuf>,
    exec: Option<String>,
    fail_fast: bool,
    threshold: Option<usize>,
) -> Result<ExitCode> {
    let root = get_root(root);
    let config = Config::load(&root)?;
    let cache = CacheManager::new(&root);

    let graph = load_graph(&root, &config, &cache, no_cache)?;

    let git = GitChangeDetector::new(root.clone())?;
    let changes = if let Some(ref since_ref) = since {
        git.detect_changes_since(since_ref)?
    } else {
        let base_ref = base.unwrap_or_else(|| git.get_default_base());
        git.detect_changes(&base_ref)?
    };

    // Find affected
    let finder = AffectedTestFinder::new(&graph);
    let result = finder.find_affected(&changes);
    let (total_tests, total_sources) = finder.get_totals();

    if let Some(max_tests) = threshold {
        if result.tests.len() > max_tests {
            eprintln!(
                "Threshold exceeded: {} affected tests > {} max. Using all tests.",
                result.tests.len(),
                max_tests
            );
            let all_tests = graph.get_test_files();
            let files: Vec<PathBuf> = all_tests
                .iter()
                .filter_map(|&id| graph.get_file_path(id).map(|p| p.to_path_buf()))
                .collect();
            return run_with_files(&files, &format, exec, fail_fast, &root, total_tests, total_sources);
        }
    }

    let files = if sources { &result.sources } else { &result.tests };
    run_with_files(files, &format, exec, fail_fast, &root, total_tests, total_sources)
}

fn run_with_files(
    files: &[PathBuf],
    format: &str,
    exec: Option<String>,
    fail_fast: bool,
    root: &PathBuf,
    total_tests: usize,
    total_sources: usize,
) -> Result<ExitCode> {
    let output_format: OutputFormat = format.parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    let files_vec: Vec<PathBuf> = files.to_vec();
    let output = match output_format {
        OutputFormat::Paths => OutputFormatter::format_paths(files),
        OutputFormat::Json => OutputFormatter::format_json(
            &files_vec,
            &files_vec,
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

        if fail_fast {
            for file in files {
                let file_str = file.to_string_lossy();
                let cmd_str = cmd_template.replace("{}", &file_str);
                
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
                    eprintln!("Test failed, stopping (--fail-fast)");
                    return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
                }
            }
        } else {
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
        }
    } else {
        println!("{}", output);
    }

    Ok(ExitCode::SUCCESS)
}

fn run_why(
    test: PathBuf,
    base: Option<String>,
    since: Option<String>,
    root: Option<PathBuf>,
    no_cache: bool,
    all: bool,
) -> Result<ExitCode> {
    let root = get_root(root);
    let config = Config::load(&root)?;
    let cache = CacheManager::new(&root);

    let graph = load_graph(&root, &config, &cache, no_cache)?;

    let test_path = if test.is_absolute() {
        test
    } else {
        root.join(&test)
    };

    let git = GitChangeDetector::new(root.clone())?;
    let changes = if let Some(ref since_ref) = since {
        git.detect_changes_since(since_ref)?
    } else {
        let base_ref = base.unwrap_or_else(|| git.get_default_base());
        git.detect_changes(&base_ref)?
    };

    if changes.is_empty() {
        eprintln!("No changes detected.");
        return Ok(ExitCode::SUCCESS);
    }

    let finder = AffectedTestFinder::new(&graph);

    let affected = finder.find_affected(&changes);
    let test_canonical = std::fs::canonicalize(&test_path).unwrap_or(test_path.clone());
    let is_affected = affected.tests.iter().any(|t| {
        std::fs::canonicalize(t).unwrap_or(t.clone()) == test_canonical
    });

    if !is_affected {
        eprintln!("Test '{}' is NOT affected by current changes.", test_path.display());
        eprintln!("\nChanged files:");
        let changed_files = changes.all_changed();
        for f in &changed_files {
            eprintln!("  - {}", f.display());
        }
        return Ok(ExitCode::SUCCESS);
    }

    println!("Test '{}' IS affected by changes.\n", test_path.display());

    if all {
        let paths = finder.find_all_paths_to_test(&test_path, &changes);
        if paths.is_empty() {
            println!("Could not trace dependency path (test may import changed file directly).");
        } else {
            println!("Dependency paths ({} found):\n", paths.len());
            for (i, path) in paths.iter().enumerate() {
                println!("  {}. {}", i + 1, path.format());
            }
        }
    } else {
        match finder.find_why(&test_path, &changes) {
            Some(path) => {
                println!("Shortest dependency path:\n");
                println!("  {}", path.format());
                println!("\nUse --all to see all paths.");
            }
            None => {
                println!("Could not trace dependency path (test may import changed file directly).");
            }
        }
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
