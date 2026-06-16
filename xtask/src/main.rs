use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
struct Fixtures {
    fixture: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    format: String,
    repo: String,
    version: String,
    pattern: String,
    extract_dir: String,
    binary: String,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("fixtures") => download_fixtures(args.next()),
        Some(command) => bail!("unknown xtask command: {command}"),
        None => {
            eprintln!("usage: cargo run -p xtask -- fixtures [format]");
            Ok(())
        }
    }
}

fn download_fixtures(filter: Option<String>) -> Result<()> {
    let root = project_root()?;
    let fixtures_dir = root.join("tests/fixtures");
    let config_path = fixtures_dir.join("fixtures.toml");
    let config: Fixtures = toml::from_str(&fs::read_to_string(&config_path)?)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    fs::create_dir_all(&fixtures_dir)?;
    println!("Test fixture downloader");
    println!("  config: {}", config_path.display());

    for fixture in config.fixture {
        if filter
            .as_deref()
            .is_some_and(|format| format != fixture.format)
        {
            continue;
        }
        download_fixture(&fixtures_dir, &fixture)?;
    }

    Ok(())
}

fn download_fixture(fixtures_dir: &Path, fixture: &Fixture) -> Result<()> {
    let binary_path = fixtures_dir
        .join(&fixture.extract_dir)
        .join(&fixture.binary);
    if binary_path.is_file() {
        println!("OK {} ({}) already exists", fixture.name, fixture.format);
        return Ok(());
    }

    println!("Downloading {} ({})", fixture.name, fixture.format);
    if let Some(parent) = binary_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let archive = fixtures_dir.join(&fixture.pattern);
    if !archive.is_file() {
        run(Command::new("gh")
            .arg("release")
            .arg("download")
            .arg(&fixture.version)
            .arg("--repo")
            .arg(&fixture.repo)
            .arg("--pattern")
            .arg(&fixture.pattern)
            .arg("--dir")
            .arg(fixtures_dir)
            .arg("--clobber"))?;
    }

    match fixture.format.as_str() {
        "macho" => extract_macho(fixtures_dir, fixture, &archive, &binary_path)?,
        "pe" => extract_pe(fixtures_dir, fixture, &archive, &binary_path)?,
        "elf" => extract_elf(fixtures_dir, fixture, &archive, &binary_path)?,
        other => bail!("unknown fixture format {other} for {}", fixture.name),
    }

    let _ = fs::remove_file(archive);
    println!("OK {} ({}) downloaded", fixture.name, fixture.format);
    Ok(())
}

fn extract_macho(
    _fixtures_dir: &Path,
    fixture: &Fixture,
    archive: &Path,
    binary_path: &Path,
) -> Result<()> {
    let target_dir = binary_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow!("invalid Mach-O binary path"))?;
    run(Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(target_dir))?;

    if binary_path.is_file() {
        return Ok(());
    }

    let binary_name = Path::new(&fixture.binary)
        .file_name()
        .ok_or_else(|| anyhow!("invalid binary name"))?;
    let found = find_file_case_insensitive(target_dir, binary_name)?;
    if let Some(found) = found {
        if let Some(parent) = binary_path.parent() {
            fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(found, binary_path)?;
        #[cfg(not(unix))]
        fs::copy(found, binary_path)?;
        return Ok(());
    }

    bail!("binary not found after extracting {}", fixture.pattern)
}

fn extract_pe(
    _fixtures_dir: &Path,
    fixture: &Fixture,
    archive: &Path,
    binary_path: &Path,
) -> Result<()> {
    require_command("7z")?;
    let temp = temp_dir("tauri-dumper-pe")?;
    run(Command::new("7z")
        .arg("x")
        .arg(archive)
        .arg(format!("-o{}", temp.display()))
        .arg("-y"))?;
    let found = find_file_case_insensitive(&temp, Path::new(&fixture.binary).as_os_str())?
        .ok_or_else(|| anyhow!("{} not found in installer", fixture.binary))?;
    fs::copy(found, binary_path)?;
    let _ = fs::remove_dir_all(temp);
    Ok(())
}

fn extract_elf(
    _fixtures_dir: &Path,
    fixture: &Fixture,
    archive: &Path,
    binary_path: &Path,
) -> Result<()> {
    match archive.extension().and_then(|ext| ext.to_str()) {
        Some("apk") => {
            require_command("unzip")?;
            let output = Command::new("unzip")
                .arg("-p")
                .arg(archive)
                .arg(&fixture.binary)
                .output()?;
            if !output.status.success() {
                bail!("{} not found in APK", fixture.binary);
            }
            fs::write(binary_path, output.stdout)?;
        }
        Some("deb") => {
            require_command("7z")?;
            let temp = temp_dir("tauri-dumper-deb")?;
            run(Command::new("7z")
                .arg("x")
                .arg(archive)
                .arg(format!("-o{}", temp.display()))
                .arg("-y"))?;
            let data_archive = fs::read_dir(&temp)?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("data.tar"))
                })
                .ok_or_else(|| anyhow!("data archive not found in DEB"))?;
            let root = temp.join("root");
            fs::create_dir_all(&root)?;
            run(Command::new("7z")
                .arg("x")
                .arg(data_archive)
                .arg(format!("-o{}", root.display()))
                .arg("-y"))?;
            let found = root.join(&fixture.binary);
            if !found.is_file() {
                bail!("{} not found in DEB", fixture.binary);
            }
            fs::copy(found, binary_path)?;
            let _ = fs::remove_dir_all(temp);
        }
        _ => bail!("unsupported ELF package type: {}", fixture.pattern),
    }
    Ok(())
}

fn project_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("xtask has no parent directory"))
}

fn run(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        bail!("command failed with status {status}: {:?}", command);
    }
    Ok(())
}

fn require_command(name: &str) -> Result<()> {
    let status = Command::new(name)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(_) => Ok(()),
        Err(_) => bail!("{name} is required to extract this fixture"),
    }
}

fn temp_dir(prefix: &str) -> Result<PathBuf> {
    let dir = env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn find_file_case_insensitive(root: &Path, file_name: &std::ffi::OsStr) -> Result<Option<PathBuf>> {
    let wanted = file_name.to_string_lossy().to_lowercase();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().to_lowercase() == wanted)
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}
