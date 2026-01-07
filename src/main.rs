use std::time::Instant;

use anyhow::Result;
use clap::Parser;

mod checksum;
mod config;
mod html;
mod page;
mod page_bundle;
mod shortcode;
mod state;
mod website;

use website::Website;

#[derive(Parser)]
pub struct Args {
    /// Fresh build: ignore state cache (do not read or write)
    #[arg(long)]
    fresh: bool,
}

/// File patterns that trigger a full site rebuild when any matching file changes.
pub static FULL_REBUILD_GLOBS: &[&str] = &["templates/**/*.html", "src/*.rs"];

fn main() -> Result<()> {
    let args = Args::parse();
    let start = Instant::now();

    let mut site = Website::init("", "config.toml", args)?;
    site.bake()?;

    println!("Done in {:.2?}", start.elapsed());

    Ok(())
}

// todos:
// - parallelize article processing
// - bufwriter?
