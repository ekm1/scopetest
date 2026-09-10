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
use scopetest::barrel::{BarrelAnalyzer};
use scopetest::workspace::{Workspace, WorkspaceKind};

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

        /// Output affected workspace packages instead of files
        #[arg(long)]
        packages: bool,

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

        /// Disable barrel import expansion (use barrel file as dependency instead of actual sources)
        #[arg(long)]
        no_barrel_expand: bool,
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

    /// Detect the monorepo setup and write a starter config
    Init {
        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,

        /// Overwrite an existing .scopetestrc.json
        #[arg(long)]
        force: bool,

        /// Print what would happen without writing anything
        #[arg(long)]
        dry_run: bool,
    },

    /// Analyze and rewrite barrel imports to direct imports
    Barrel {
        #[command(subcommand)]
        action: BarrelAction,
    },
}

#[derive(Subcommand)]
enum BarrelAction {
    /// Analyze a barrel file and show its exports
    Analyze {
        /// Path to the barrel file (index.ts)
        file: PathBuf,
        
        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,
    },
    
    /// Find all barrel files in a directory
    Find {
        /// Directory to search
        #[arg(short, long)]
        dir: Option<PathBuf>,
        
        /// Project root directory
        #[arg(short, long)]
        root: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Affected { 
            base, since, format, sources, packages, no_cache, root, exec, fail_fast, threshold, no_barrel_expand 
        } => {
            run_affected(base, since, format, sources, packages, no_cache, root, exec, fail_fast, threshold, no_barrel_expand)
        }
        Commands::Why { test, base, since, root, no_cache, all } => {
            run_why(test, base, since, root, no_cache, all)
        }
        Commands::Build { root } => {
            run_build(root)
        }
        Commands::Init { root, force, dry_run } => {
            run_init(root, force, dry_run)
        }
        Commands::Barrel { action } => {
            run_barrel(action)
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
            Ok(Some(mut g)) => {
                let builder = GraphBuilder::new(root.clone(), config.clone());
                let updated = builder.update_incremental(&mut g)?;
                if updated > 0 {
                    let _ = cache.save(&g);
                }
                Ok(g)
            }
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
    packages: bool,
    no_cache: bool,
    root: Option<PathBuf>,
    exec: Option<String>,
    fail_fast: bool,
    threshold: Option<usize>,
    no_barrel_expand: bool,
) -> Result<ExitCode> {
    let root = get_root(root);
    let mut config = Config::load(&root)?;
    config.expand_barrels = !no_barrel_expand;
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

    let output_format: OutputFormat = format.parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    if packages || output_format.implies_packages() {
        let workspace = Workspace::discover(&root);
        let total_packages = workspace.packages.len();

        let over_threshold = threshold.map(|max| result.tests.len() > max).unwrap_or(false);

        let names = if over_threshold {
            eprintln!(
                "Threshold exceeded: {} affected tests > {} max. Using all packages.",
                result.tests.len(),
                threshold.unwrap_or(0)
            );
            let mut all: Vec<String> =
                workspace.packages.iter().map(|p| p.name.clone()).collect();
            all.sort();
            all.dedup();
            all
        } else {
            let mut affected_files = result.tests.clone();
            affected_files.extend(result.sources.clone());
            workspace.packages_for_files(&affected_files)
        };

        return run_with_packages(&names, output_format, exec, &root, total_packages);
    }

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

const CONFIG_FILE: &str = ".scopetestrc.json";

/// Starter config. Deliberately minimal: every key here is already the
/// default, so it serves as a discoverable template rather than a set of
/// overrides the user did not ask for.
fn starter_config() -> String {
    let config = serde_json::json!({
        "testPatterns": [
            "**/*.spec.ts",
            "**/*.spec.tsx",
            "**/*.test.ts",
            "**/*.test.tsx"
        ],
        "ignorePatterns": [
            "**/node_modules/**",
            "**/dist/**",
            "**/build/**",
            "**/coverage/**"
        ],
        "extensions": [".ts", ".tsx", ".js", ".jsx"],
        "expandBarrels": true
    });

    serde_json::to_string_pretty(&config).unwrap_or_default()
}

/// The command that wires scopetest into whichever runner the repo already uses.
fn integration_snippet(kind: WorkspaceKind) -> String {
    match kind {
        WorkspaceKind::Nx => {
            "  nx run-many -t test -p $(scopetest affected -f nx)".to_string()
        }
        WorkspaceKind::Turborepo => {
            "  turbo run test $(scopetest affected -f turbo)".to_string()
        }
        WorkspaceKind::Pnpm => {
            "  pnpm $(scopetest affected -f turbo) test".to_string()
        }
        WorkspaceKind::Lerna | WorkspaceKind::NpmWorkspaces => {
            "  npm test --workspace $(scopetest affected --packages)".to_string()
        }
        WorkspaceKind::Single => {
            "  scopetest affected -x \"jest --runTestsByPath {}\"".to_string()
        }
    }
}

fn run_init(root: Option<PathBuf>, force: bool, dry_run: bool) -> Result<ExitCode> {
    let root = get_root(root);
    let workspace = Workspace::discover(&root);

    println!("scopetest init");
    println!();
    println!("  root:      {}", root.display());
    println!("  detected:  {}", workspace.kind.label());

    if workspace.packages.is_empty() {
        println!("  packages:  none found");
    } else {
        println!("  packages:  {}", workspace.packages.len());
        for package in workspace.packages.iter().take(5) {
            let relative = package
                .root
                .strip_prefix(&root)
                .unwrap_or(&package.root)
                .display()
                .to_string();
            let location = if relative.is_empty() { ".".to_string() } else { relative };
            println!("             - {} ({})", package.name, location);
        }
        if workspace.packages.len() > 5 {
            println!("             ... and {} more", workspace.packages.len() - 5);
        }
    }

    println!();

    let config_path = root.join(CONFIG_FILE);
    if config_path.exists() && !force {
        println!("  {} already exists (use --force to overwrite)", CONFIG_FILE);
    } else if dry_run {
        println!("  would write {}", CONFIG_FILE);
    } else {
        std::fs::write(&config_path, starter_config())?;
        println!("  wrote {}", CONFIG_FILE);
    }

    println!();
    println!("Run your affected tests with:");
    println!();
    println!("{}", integration_snippet(workspace.kind));
    println!();

    if workspace.kind != WorkspaceKind::Single && !workspace.is_monorepo() {
        eprintln!(
            "Warning: {} detected but only {} package(s) found. Check your workspace globs.",
            workspace.kind.label(),
            workspace.packages.len()
        );
    }

    Ok(ExitCode::SUCCESS)
}

fn run_with_packages(
    packages: &[String],
    format: OutputFormat,
    exec: Option<String>,
    root: &PathBuf,
    total_packages: usize,
) -> Result<ExitCode> {
    let output = OutputFormatter::format_packages(packages, format, total_packages);

    if let Some(cmd_template) = exec {
        if packages.is_empty() {
            eprintln!("No affected packages found.");
            return Ok(ExitCode::SUCCESS);
        }

        let cmd_str = cmd_template.replace("{}", &output);
        eprintln!("Running: {}", cmd_str);

        let status = Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .current_dir(root)
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
        // Package-shaped formats are handled by run_with_packages before we
        // ever get here; fall back to plain paths rather than panicking.
        OutputFormat::Turbo | OutputFormat::Nx => OutputFormatter::format_paths(files),
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


fn run_barrel(action: BarrelAction) -> Result<ExitCode> {
    match action {
        BarrelAction::Analyze { file, root } => {
            let root = get_root(root);
            let mut analyzer = BarrelAnalyzer::new(root);
            
            let file_path = if file.is_absolute() {
                file
            } else {
                std::env::current_dir()?.join(&file)
            };
            
            let exports = analyzer.analyze_barrel(&file_path);
            
            if exports.is_empty() {
                eprintln!("No exports found in {}", file_path.display());
                return Ok(ExitCode::SUCCESS);
            }
            
            println!("Exports from {}:\n", file_path.display());
            
            let mut sorted: Vec<_> = exports.iter().collect();
            sorted.sort_by_key(|(name, _)| *name);
            
            for (name, source) in sorted {
                let source_display = source.source_path.display();
                if let Some(ref orig) = source.original_name {
                    println!("  {} (as {}) <- {}", name, orig, source_display);
                } else {
                    println!("  {} <- {}", name, source_display);
                }
            }
            
            println!("\nTotal: {} exports", exports.len());
            Ok(ExitCode::SUCCESS)
        }
        
        BarrelAction::Find { dir, root } => {
            let root = get_root(root);
            let search_dir = dir.unwrap_or(root.clone());
            let mut analyzer = BarrelAnalyzer::new(root);
            
            let barrels = analyzer.find_barrels(&search_dir);
            
            if barrels.is_empty() {
                eprintln!("No barrel files found in {}", search_dir.display());
                return Ok(ExitCode::SUCCESS);
            }
            
            println!("Found {} barrel files:\n", barrels.len());
            
            for barrel in &barrels {
                let exports = analyzer.analyze_barrel(barrel);
                let barrel_canonical = std::fs::canonicalize(barrel).unwrap_or(barrel.clone());
                let re_exports: usize = exports.values()
                    .filter(|s| s.source_path != barrel_canonical)
                    .count();
                
                if re_exports > 0 {
                    println!("  {} ({} re-exports)", barrel.display(), re_exports);
                }
            }
            
            Ok(ExitCode::SUCCESS)
        }
    }
}
