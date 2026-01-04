use std::time::Instant;

use anyhow::Result;

mod checksum;
mod config;
mod html;
mod page;
mod page_bundle;
mod state;
mod website;

use website::Website;

/// File patterns that trigger a full site rebuild when any matching file changes.
pub static FULL_REBUILD_GLOBS: &[&str] = &["templates/*.html", "src/*.rs"];

fn main() -> Result<()> {
    let start = Instant::now();

    let mut site = Website::init("", "config.toml")?;
    site.bake()?;

    println!("Done in {:.2?}", start.elapsed());

    Ok(())
}

// todos:
// - parallelize article processing
// - bufwriter?
