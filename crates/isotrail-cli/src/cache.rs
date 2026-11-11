use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

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
    /// Delete cached downloads; map data stays cached until cleared
    Clear {
        /// Don't ask for confirmation
        #[arg(short, long)]
        yes: bool,

        /// Only delete downloads made more than this many days ago
        #[arg(long, value_name = "DAYS")]
        older_than: Option<u32>,
    },
}

pub fn run(args: &CacheArgs) -> anyhow::Result<()> {
    let cache = Cache::new(dir()?);
    match args.command {
        CacheCommand::Path => println!("{}", cache.dir().display()),
        CacheCommand::Clear { yes, older_than } => clear(&cache, yes, older_than)?,
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

fn clear(cache: &Cache, yes: bool, older_than: Option<u32>) -> anyhow::Result<()> {
    let dir = cache.dir().display();
    let age = older_than.map(|days| Duration::from_secs(u64::from(days) * 86_400));
    let stats = cache.stats(age)?;
    let left = left_alone(stats.others);
    if stats.files == 0 {
        match older_than {
            Some(days) => println!(
                "Nothing in the cache at {dir} is older than {}{left}",
                plural(days.into(), "day")
            ),
            None => println!("The cache at {dir} is empty{left}"),
        }
        return Ok(());
    }
    let older = older_than.map_or(String::new(), |days| {
        format!(" downloaded more than {} ago", plural(days.into(), "day"))
    });
    let summary = format!("{}{older} in {dir}", describe(stats));
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
    let cleared = cache.clear(age)?;
    println!(
        "Deleted {}{older}{}",
        describe(cleared),
        left_alone(cleared.others)
    );
    Ok(())
}

fn describe(stats: CacheStats) -> String {
    format!("{} ({})", plural(stats.files, "file"), size(stats.bytes))
}

fn plural(count: u64, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// Files in the cache folder that isotrail didn't write
fn left_alone(others: u64) -> String {
    match others {
        0 => String::new(),
        n => format!("; left {} alone", plural(n, "other file")),
    }
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
            others: 0,
        };
        assert_eq!(describe(one), "1 file (10 B)");
        let many = CacheStats {
            files: 4,
            bytes: 2_000,
            others: 3,
        };
        assert_eq!(describe(many), "4 files (2.0 kB)");
        assert_eq!(left_alone(0), "");
        assert_eq!(left_alone(1), "; left 1 other file alone");
        assert_eq!(left_alone(3), "; left 3 other files alone");
    }
}
