use anyhow::{anyhow, Result};
use clap::Parser;
use normalize_path::NormalizePath;
use tauri_dumper::Dumper;
use std::fs::{self, File};
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long)]
    input: String,

    #[arg(short, long)]
    output: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let file = File::open(&args.input)?;
    let dumper = Dumper::new(file)?;

    println!("Scanning for assets...");
    let assets = dumper.scan_assets()?;
    println!("Scanning completed. Found {} assets", assets.len());

    if assets.is_empty() {
        return Err(anyhow!("No assets found"));
    }

    for asset in assets {
        let decompressed = dumper.decompress_asset(&asset)?;

        // Remove leading '/'
        let output = Path::new(&args.output).normalize();
        let path = output.join(&asset.name[1..]).normalize();

        // Sanitize path to prevent traversal attacks
        if !path.starts_with(&output) {
            return Err(anyhow!("Path traversal found: {:?}", path));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        println!("Dump asset: {}, size: {:#X}", asset.name, asset.data.len());
        fs::write(path, decompressed)?;
    }

    println!("Done :)");

    Ok(())
}
