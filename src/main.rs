use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result};
use gray_matter::{engine::YAML, Matter};
use lazy_static::lazy_static;
use regex::Regex;
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

// Function to extract image references from markdown content
fn extract_image_references(content: &str) -> Vec<String> {
    let re = Regex::new("!\\[[^\\]]*\\]\\(([^\\)]+)\\)").unwrap();
    re.captures_iter(content)
        .map(|capture| capture[1].to_string())
        .collect()
}

// Function to copy referenced images to the website directory
fn copy_images(image_references: &[String]) -> Result<()> {
    for image_reference in image_references {
        let image_path = CONTENT_DIR.join(image_reference);
        let file_name = image_path.file_name().unwrap();
        let destination_path = WEBSITE_DIR.join(file_name);
        fs::copy(image_path, &destination_path)?;
    }
    Ok(())
}

// Function to update image references in HTML content
fn update_image_references(html_content: &str, image_references: &[String]) -> String {
    let mut updated_content = html_content.to_string();
    for image_reference in image_references {
        let file_name = Path::new(&image_reference)
            .file_name()
            .unwrap()
            .to_string_lossy();
        let new_image_path = format!("{}{}", "website/", file_name);
        updated_content = updated_content.replace(image_reference, &new_image_path);
    }
    updated_content
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

            // Extract image references from the markdown content
            let image_references = extract_image_references(&contents);

            // Copy referenced images to the website directory
            copy_images(&image_references)?;

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
                let mut context = HashMap::new();
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
        let mut context = HashMap::new();
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
