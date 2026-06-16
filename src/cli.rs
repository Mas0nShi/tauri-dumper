use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use comfy_table::{presets::UTF8_FULL, Table};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri_dumper::asset::AssetTableSummary;
use tauri_dumper::binary::BinaryKind;
use tauri_dumper::{
    AssetScanner, BinaryImage, ExportOptions, ExportSummary, RepackSummary, Repacker,
};

#[derive(Parser, Debug)]
#[command(author, version, about = "Extract and replace embedded Tauri assets")]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(value_name = "BINARY")]
    binary: Option<PathBuf>,

    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    #[command(flatten)]
    common: CommonArgs,

    #[command(flatten)]
    extract: ExtractFlags,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Extract(ExtractCommand),
    List(ReadCommand),
    Inspect(ReadCommand),
    Verify(ReadCommand),
    Repack(RepackCommand),
}

#[derive(Args, Debug, Clone)]
struct CommonArgs {
    #[arg(long)]
    json: bool,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    quiet: bool,
}

#[derive(Args, Debug, Clone)]
struct FilterArgs {
    #[arg(long = "include", value_name = "GLOB")]
    include: Vec<String>,

    #[arg(long = "exclude", value_name = "GLOB")]
    exclude: Vec<String>,
}

#[derive(Args, Debug, Clone)]
struct ExtractFlags {
    #[arg(long, conflicts_with = "skip_existing")]
    overwrite: bool,

    #[arg(long, conflicts_with = "overwrite")]
    skip_existing: bool,

    #[arg(long)]
    dry_run: bool,

    #[command(flatten)]
    filter: FilterArgs,
}

#[derive(Args, Debug)]
struct ExtractCommand {
    #[arg(value_name = "BINARY")]
    binary: PathBuf,

    #[arg(short, long, value_name = "DIR", default_value = "output")]
    output: PathBuf,

    #[command(flatten)]
    common: CommonArgs,

    #[command(flatten)]
    flags: ExtractFlags,
}

#[derive(Args, Debug)]
struct ReadCommand {
    #[arg(value_name = "BINARY")]
    binary: PathBuf,

    #[command(flatten)]
    common: CommonArgs,
}

#[derive(Args, Debug)]
struct RepackCommand {
    #[arg(value_name = "BINARY")]
    binary: PathBuf,

    #[arg(long, value_name = "DIR")]
    assets: PathBuf,

    #[arg(short, long, value_name = "BINARY")]
    output: PathBuf,

    #[arg(long)]
    strict: bool,

    #[arg(long)]
    skip_oversized: bool,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    allow_source_mismatch: bool,

    #[arg(long)]
    ad_hoc_sign: bool,

    #[command(flatten)]
    common: CommonArgs,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Extract(command)) => extract(
            &command.binary,
            &command.output,
            &command.flags,
            &command.common,
        ),
        Some(Commands::List(command)) => list(&command.binary, &command.common),
        Some(Commands::Inspect(command)) => inspect(&command.binary, &command.common),
        Some(Commands::Verify(command)) => verify(&command.binary, &command.common),
        Some(Commands::Repack(command)) => repack(command),
        None => {
            let binary = cli
                .binary
                .context("missing binary path; run `tauri-dumper --help` for usage")?;
            let output = cli.output.unwrap_or_else(|| PathBuf::from("output"));
            extract(&binary, &output, &cli.extract, &cli.common)
        }
    }
}

fn extract(binary: &Path, output: &Path, flags: &ExtractFlags, common: &CommonArgs) -> Result<()> {
    let (_image, table) = scan(binary, common)?;
    ensure_assets_found(&table)?;

    let spinner = spinner(common, "exporting assets");
    let mut options = ExportOptions::new(output);
    options.overwrite = !flags.skip_existing;
    options.skip_existing = flags.skip_existing;
    options.dry_run = flags.dry_run;
    options = options
        .include_globs(&flags.filter.include)?
        .exclude_globs(&flags.filter.exclude)?;
    let summary = table.export(&options)?;
    finish_spinner(spinner);

    if common.json {
        print_json(&summary)
    } else if !common.quiet {
        print_export_summary(&summary);
        Ok(())
    } else {
        Ok(())
    }
}

fn list(binary: &Path, common: &CommonArgs) -> Result<()> {
    let (_image, table) = scan(binary, common)?;
    ensure_assets_found(&table)?;

    if common.json {
        print_json(&table.summary())
    } else if !common.quiet {
        print_asset_tree(&table);
        Ok(())
    } else {
        Ok(())
    }
}

fn inspect(binary: &Path, common: &CommonArgs) -> Result<()> {
    let (_image, table) = scan(binary, common)?;
    let summary = table.summary();

    if common.json {
        print_json(&summary)
    } else if !common.quiet {
        print_inspect_summary(&summary);
        Ok(())
    } else {
        Ok(())
    }
}

fn verify(binary: &Path, common: &CommonArgs) -> Result<()> {
    let (_image, table) = scan(binary, common)?;
    ensure_assets_found(&table)?;

    #[derive(Serialize)]
    struct VerifySummary {
        ok: bool,
        asset_count: usize,
        binary: tauri_dumper::binary::BinaryMetadata,
    }

    let summary = VerifySummary {
        ok: true,
        asset_count: table.len(),
        binary: table.metadata().clone(),
    };

    if common.json {
        print_json(&summary)
    } else if !common.quiet {
        println!("OK: found {} valid embedded assets", table.len());
        Ok(())
    } else {
        Ok(())
    }
}

fn repack(command: RepackCommand) -> Result<()> {
    let (image, table) = scan(&command.binary, &command.common)?;
    ensure_assets_found(&table)?;
    let binary_kind = table.metadata().kind;

    let spinner = spinner(&command.common, "repacking assets");
    let summary = Repacker::new(image, table)
        .replace_from_dir(&command.assets)
        .strict(command.strict)
        .skip_oversized(command.skip_oversized)
        .dry_run(command.dry_run)
        .allow_source_mismatch(command.allow_source_mismatch)
        .write(&command.output)?;
    finish_spinner(spinner);

    if command.ad_hoc_sign && !command.dry_run && cfg!(target_os = "macos") {
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&command.output)
            .status()
            .context("failed to run codesign")?;
    }

    if command.common.json {
        print_json(&summary)
    } else if !command.common.quiet {
        print_repack_summary(&summary);
        if binary_kind == BinaryKind::MachO && !command.ad_hoc_sign && !command.dry_run {
            println!(
                "macOS note: run `codesign --force --deep --sign - {}` before launching.",
                command.output.display()
            );
        }
        Ok(())
    } else {
        Ok(())
    }
}

fn scan(binary: &Path, common: &CommonArgs) -> Result<(BinaryImage, tauri_dumper::AssetTable)> {
    let spinner = spinner(common, "scanning binary");
    let image = BinaryImage::open(binary)
        .with_context(|| format!("failed to open {}", binary.display()))?;
    let table = AssetScanner::scan(&image)?;
    finish_spinner(spinner);
    Ok((image, table))
}

fn ensure_assets_found(table: &tauri_dumper::AssetTable) -> Result<()> {
    if table.is_empty() {
        anyhow::bail!("no embedded Tauri assets found");
    }
    Ok(())
}

fn spinner(common: &CommonArgs, message: &'static str) -> Option<ProgressBar> {
    if common.quiet || common.json {
        return None;
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message(message);
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    Some(pb)
}

fn finish_spinner(spinner: Option<ProgressBar>) {
    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[derive(Default)]
struct AssetTreeNode {
    children: BTreeMap<String, AssetTreeNode>,
    asset: Option<AssetTreeLeaf>,
}

#[derive(Clone, Copy)]
struct AssetTreeLeaf {
    compressed_size: usize,
    decompressed_size: usize,
}

fn print_asset_tree(table: &tauri_dumper::AssetTable) {
    let mut root = AssetTreeNode::default();
    for asset in table.assets() {
        root.insert(
            asset.name(),
            AssetTreeLeaf {
                compressed_size: asset.compressed_size(),
                decompressed_size: asset.decompressed_size(),
            },
        );
    }

    println!("Assets: {}", table.len());
    print_asset_tree_children(&root, "");
}

impl AssetTreeNode {
    fn insert(&mut self, path: &str, asset: AssetTreeLeaf) {
        let mut node = self;
        for component in path
            .trim_start_matches('/')
            .split('/')
            .filter(|component| !component.is_empty())
        {
            node = node.children.entry(component.to_string()).or_default();
        }
        node.asset = Some(asset);
    }
}

fn print_asset_tree_children(node: &AssetTreeNode, prefix: &str) {
    let child_count = node.children.len();
    for (index, (name, child)) in node.children.iter().enumerate() {
        let is_last = index + 1 == child_count;
        let connector = if is_last {
            "\u{2514}\u{2500}\u{2500} "
        } else {
            "\u{251c}\u{2500}\u{2500} "
        };
        println!(
            "{}{}{}{}",
            prefix,
            connector,
            name,
            asset_tree_size_suffix(child.asset)
        );

        let next_prefix = if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}\u{2502}   ")
        };
        print_asset_tree_children(child, &next_prefix);
    }
}

fn asset_tree_size_suffix(asset: Option<AssetTreeLeaf>) -> String {
    asset.map_or_else(String::new, |asset| {
        format!(
            " ({} compressed, {} decompressed)",
            format_bytes(asset.compressed_size),
            format_bytes(asset.decompressed_size)
        )
    })
}

fn format_bytes(bytes: usize) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

fn print_inspect_summary(summary: &AssetTableSummary) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Field", "Value"]);
    table.add_row(vec!["Format", &format!("{:?}", summary.binary.kind)]);
    table.add_row(vec!["Architecture", &summary.binary.architecture]);
    table.add_row(vec!["File size", &summary.binary.file_size.to_string()]);
    table.add_row(vec!["SHA-256", &summary.binary.sha256]);
    table.add_row(vec!["Assets", &summary.asset_count.to_string()]);
    table.add_row(vec![
        "Compressed bytes",
        &summary.total_compressed_size.to_string(),
    ]);
    table.add_row(vec![
        "Decompressed bytes",
        &summary.total_decompressed_size.to_string(),
    ]);
    println!("{table}");
}

fn print_export_summary(summary: &ExportSummary) {
    println!("Export complete");
    println!("  output: {}", summary.output_dir.display());
    println!("  exported: {}", summary.exported);
    println!("  skipped existing: {}", summary.skipped_existing);
    println!("  skipped by filter: {}", summary.skipped_filter);
    if summary.dry_run {
        println!("  dry run: no files were written");
    }
}

fn print_repack_summary(summary: &RepackSummary) {
    println!("Repack complete");
    if let Some(output) = &summary.output {
        println!("  output: {}", output.display());
    }
    println!("  replaced: {}", summary.replaced);
    println!("  unchanged: {}", summary.unchanged);
    println!("  skipped oversized: {}", summary.skipped_oversized);
    println!(
        "  unsupported additions: {}",
        summary.unsupported_additions.len()
    );
    if summary.dry_run {
        println!("  dry run: no binary was written");
    }
    for oversized in &summary.oversized {
        println!(
            "  oversized: {} ({} > {}, +{})",
            oversized.asset,
            oversized.new_compressed_size,
            oversized.original_compressed_size,
            oversized.delta
        );
    }
}
