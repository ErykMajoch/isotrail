use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use isotrail_geo::{Cache, CacheStats};

const DIR_ENV: &str = "ISOTRAIL_CACHE_DIR";

#[derive(Debug, Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    command: CacheCommand,
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Print the cache directory (set ISOTRAIL_CACHE_DIR to use another one)
    Path,
    /// Delete every cached download
    Clear {
        /// Don't ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub fn run(args: &CacheArgs) -> anyhow::Result<()> {
    let cache = Cache::new(dir()?);
    match args.command {
        CacheCommand::Path => println!("{}", cache.dir().display()),
        CacheCommand::Clear { yes } => clear(&cache, yes)?,
    }
    Ok(())
}

pub fn dir() -> anyhow::Result<PathBuf> {
    env::var_os(DIR_ENV)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(Cache::default_dir)
        .with_context(|| format!("could not find a cache directory for this user; set {DIR_ENV}"))
}

fn clear(cache: &Cache, yes: bool) -> anyhow::Result<()> {
    let dir = cache.dir().display();
    let stats = cache.stats()?;
    if stats.files == 0 {
        println!("The cache at {dir} is empty");
        return Ok(());
    }
    let summary = format!("{} in {dir}", describe(stats));
    if !yes {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            bail!("not deleting {summary} without confirmation; pass --yes");
        }
        eprint!("Delete {summary}? [y/N] ");
        io::stderr().flush()?;
        let mut answer = String::new();
        stdin.lock().read_line(&mut answer)?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Nothing deleted");
            return Ok(());
        }
    }
    let cleared = cache.clear()?;
    println!("Deleted {}", describe(cleared));
    Ok(())
}

fn describe(stats: CacheStats) -> String {
    let files = if stats.files == 1 { "file" } else { "files" };
    format!("{} {files} ({})", stats.files, size(stats.bytes))
}

fn size(bytes: u64) -> String {
    let bytes = bytes as f64;
    match bytes {
        b if b < 1e3 => format!("{b} B"),
        b if b < 1e6 => format!("{:.1} kB", b / 1e3),
        b if b < 1e9 => format!("{:.1} MB", b / 1e6),
        b => format!("{:.2} GB", b / 1e9),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(48_213), "48.2 kB");
        assert_eq!(size(3_250_000), "3.2 MB");
        assert_eq!(size(1_500_000_000), "1.50 GB");
    }

    #[test]
    fn descriptions() {
        let one = CacheStats {
            files: 1,
            bytes: 10,
        };
        assert_eq!(describe(one), "1 file (10 B)");
        let many = CacheStats {
            files: 4,
            bytes: 2_000,
        };
        assert_eq!(describe(many), "4 files (2.0 kB)");
    }
}
