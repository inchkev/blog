use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result};
use gray_matter::{engine::YAML, Matter};
use lazy_static::lazy_static;
use serde::Deserialize;
use tinytemplate::TinyTemplate;
use walkdir::WalkDir;

lazy_static! {
    static ref CONTENT_DIR: &'static Path = Path::new("content");
    static ref TEMPLATE_DIR: &'static Path = Path::new("templates");
    static ref WEBSITE_DIR: &'static Path = Path::new("website");
    static ref PAGE_TEMPLATE: String = fs::read_to_string(TEMPLATE_DIR.join("page.html")).unwrap();
    static ref INDEX_TEMPLATE: String =
        fs::read_to_string(TEMPLATE_DIR.join("index.html")).unwrap();
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct FrontMatter {
    title: String,
    date: String,
    slug: Option<String>,
    draft: bool,
}

fn main() -> Result<()> {
    let mut tt = TinyTemplate::new();
    tt.add_template("page", &PAGE_TEMPLATE).unwrap();
    tt.add_template("index", &INDEX_TEMPLATE).unwrap();

    let mut posts = Vec::new();

    for entry in WalkDir::new(*CONTENT_DIR)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() && entry.path().extension() == Some(OsStr::new("md")) {
            let content =
                fs::read_to_string(entry.path()).context("Failed to read markdown file")?;

            let yaml_matter = Matter::<YAML>::new();
            let result = yaml_matter.parse(&content);
            let front_matter: FrontMatter = result.data.unwrap().deserialize().unwrap();

            // if front_matter.draft {
            //     continue;
            // }

            let contents = result.content;

            let html_contents = markdown::to_html_with_options(
                &contents,
                &markdown::Options {
                    compile: markdown::CompileOptions {
                        allow_dangerous_html: true,
                        allow_dangerous_protocol: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();

            let slug = front_matter.slug.unwrap_or_else(|| {
                entry
                    .path()
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .split('_')
                    .skip(1)
                    .collect()
            });

            let post_context = {
                let mut context = std::collections::HashMap::new();
                context.insert("title", front_matter.title.clone());
                context.insert("slug", slug.clone());
                context.insert("date", front_matter.date.clone());
                context.insert("contents", html_contents);
                context
            };

            let rendered = tt
                .render("page", &post_context)
                .context("Failed to render page template")?;
            let output_path = WEBSITE_DIR.join(format!("{}.html", slug));
            let mut output_file = File::create(output_path).unwrap();
            output_file.write_all(rendered.as_bytes()).unwrap();

            posts.push(post_context);
        }
    }

    let index_context = {
        let mut context = std::collections::HashMap::new();
        context.insert("posts", posts);
        context
    };

    let rendered_index = tt
        .render("index", &index_context)
        .context("Failed to render index template")?;
    let index_path = WEBSITE_DIR.join("index.html");
    let mut index_file = File::create(index_path).unwrap();
    index_file.write_all(rendered_index.as_bytes()).unwrap();

    Ok(())
}
