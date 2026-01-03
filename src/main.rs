use std::env;
use std::time::Instant;

use anyhow::Result;

mod checksum;
mod html;
mod state;
mod types;
mod website;

use website::Website;

fn main() -> Result<()> {
    let start = Instant::now();

    let current_dir = env::current_dir()?;
    let mut site = Website::init(current_dir)?;
    site.bake()?;

    println!("Done in {:.2?}", start.elapsed());

    Ok(())
}
