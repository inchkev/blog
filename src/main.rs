use anyhow::Result;
use std::{
    fs::{
        self,
        File,
    },
    io::{
        Read,
        Write,
    },
    path::Path,
};

const CONTENT_DIR: &str = "content";

fn save_html(s: &String) -> Result<()> {
    let mut file = File::create("index.html")?;
    file.write_all(s.as_bytes())?;
    Ok(())
}

fn main() {
    // open file
    // let output = fs::read_dir(CONTENT_DIR).unwrap();

    let content_dir = Path::new(CONTENT_DIR);

    let test_html = content_dir.join("test.md");

    let mut file = fs::File::open(test_html).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let html = markdown::to_html(&contents);
    save_html(&html).unwrap();
}
