use std::{
    cmp::Reverse,
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::Result;
use gray_matter::ParsedEntity;
use kuchikiki::traits::TendrilSink;
use markdown::{CompileOptions, ParseOptions};
use walkdir::WalkDir;

mod html;
mod state;
mod types;
use state::{calculate_sha256_hash, StateManager};
use types::{FrontPageInfo, PageFrontMatter};

static CONTENT_DIR: LazyLock<PathBuf> = LazyLock::new(|| "content".into());
static TEMPLATE_DIR: LazyLock<PathBuf> = LazyLock::new(|| "templates".into());
static THEME_DIR: LazyLock<PathBuf> = LazyLock::new(|| "themes".into());
static WEBSITE_DIR: LazyLock<PathBuf> = LazyLock::new(|| "website".into());
static STATE_FILE: LazyLock<PathBuf> = LazyLock::new(|| "state.json".into());

fn yaml_matter() -> &'static gray_matter::Matter<gray_matter::engine::YAML> {
    use gray_matter::{engine::YAML, Matter};
    static MATTER: LazyLock<Matter<YAML>> = LazyLock::new(Matter::<YAML>::new);
    &MATTER
}

fn tera() -> &'static tera::Tera {
    static TERA: LazyLock<tera::Tera> = LazyLock::new(|| {
        let mut tera = tera::Tera::new(&TEMPLATE_DIR.join("*.html").to_string_lossy()).unwrap();
        tera.autoescape_on(vec![]);
        tera
    });
    &TERA
}

fn ts() -> &'static syntect::highlighting::ThemeSet {
    static PS: LazyLock<syntect::highlighting::ThemeSet> =
        LazyLock::new(|| syntect::highlighting::ThemeSet::load_from_folder(&*THEME_DIR).unwrap());
    &PS
}

fn process_html<P: AsRef<Path>>(html: &str, page_dir: P) -> (String, bool) {
    let document = kuchikiki::parse_html().one(html);

    html::copy_media_and_add_dimensions(&document, page_dir);
    let has_code_blocks = html::has_code_blocks(&document);
    if has_code_blocks {
        html::syntax_highlight_code_blocks(&document);
    }
    html::update_references_section(&document);

    (html::finish(&document), has_code_blocks)
}

#[allow(dead_code)]
fn load_syntax_theme(theme: &str) -> Result<()> {
    let theme = &ts().themes[theme];
    let css = syntect::html::css_for_theme_with_class_style(theme, html::SYNTECT_CLASSSTYLE)?;

    let css_path = WEBSITE_DIR.join("syntax.css");
    let mut css_file = File::create(css_path)?;
    css_file.write_all(css.as_bytes())?;

    Ok(())
}

fn try_get_slug_from_path<P: AsRef<Path>>(path: P) -> Option<String> {
    let stem = path.as_ref().file_stem()?.to_str()?;
    let (_date, slug) = stem.split_once('_')?;
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_owned())
    }
}

fn main() -> Result<()> {
    let mut posts = Vec::<FrontPageInfo>::new();
    let mut state = StateManager::from_state_file(&*STATE_FILE).unwrap_or_default();
    let markdown_options = markdown::Options {
        parse: ParseOptions::gfm(),
        compile: CompileOptions {
            allow_dangerous_html: true,
            allow_dangerous_protocol: true,
            gfm_footnote_label: Some("References".into()),
            gfm_footnote_back_label: Some("Jump up".into()),
            ..CompileOptions::gfm()
        },
    };

    // Walk files from newest to oldest creation time
    for entry in WalkDir::new(&*CONTENT_DIR)
        .max_depth(1)
        .sort_by_key(|entry| Reverse(entry.metadata().ok().and_then(|m| m.created().ok())))
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let path = entry.into_path();
        if path.is_file() && path.extension().is_some_and(|s| s == "md") {
            print!("READ {} ... ", path.as_os_str().to_string_lossy());
            std::io::stdout().flush()?;

            let file_contents = fs::read_to_string(&path)?;
            let parsed_file: ParsedEntity = yaml_matter().parse(&file_contents)?;
            let Some(front_matter_data) = parsed_file.data else {
                println!("skipped (no data)");
                continue;
            };
            let front_matter = front_matter_data.deserialize::<PageFrontMatter>()?;
            if front_matter.draft() {
                println!("skipped (draft)");
                continue;
            }

            let slug = if let Some(s) = front_matter.slug() {
                s.to_string()
            } else {
                let Some(s) = try_get_slug_from_path(&path) else {
                    println!("skipped (no slug)");
                    continue;
                };
                s
            };

            let front_page_info = FrontPageInfo::new(
                front_matter.title().unwrap_or(&slug),
                front_matter.date(),
                slug.clone(),
            );

            // Skip if contents haven't changed
            let file_checksum = calculate_sha256_hash(&file_contents);
            if !state.contents_changed(&slug, &file_checksum) {
                state.add_or_keep(slug, file_checksum.to_string());
                posts.push(front_page_info);
                println!("skipped (no changes)");
                continue;
            }

            let html_contents =
                markdown::to_html_with_options(&parsed_file.content, &markdown_options).unwrap();

            // Create directory for page
            let page_dir = WEBSITE_DIR.join(&slug);
            if page_dir.try_exists().is_ok_and(|exists| !exists) {
                fs::create_dir(WEBSITE_DIR.join(&slug)).unwrap();
            }

            let mut post_context = front_page_info.to_map();

            // - re-formats the generated html
            // - copies images to each page's directory
            // - and more. see function
            let (html_contents, has_code_blocks) = process_html(&html_contents, &page_dir);

            post_context.insert("contents", html_contents.into());
            post_context.insert("hascodeblock", has_code_blocks.into());
            post_context.extend(
                front_matter
                    .all_else()
                    .iter()
                    .map(|(k, v)| (k.as_ref(), v.as_ref().into())),
            );

            // Render article page
            let rendered =
                tera().render("page.html", &tera::Context::from_serialize(post_context)?)?;

            let output_path = page_dir.join("index.html");
            let mut output_file = File::create(&output_path)?;
            output_file.write_all(rendered.as_bytes())?;

            println!("generated");
            println!("  WRITE {}", output_path.as_os_str().to_string_lossy());

            state.add_or_keep(slug, file_checksum.to_string());
            posts.push(front_page_info);
        }
    }

    // Delete stale files
    for slug in &state.get_stale_slugs() {
        fs::remove_dir_all(WEBSITE_DIR.join(slug)).unwrap();
    }
    // Save new state file
    state.write_state_file(&*STATE_FILE)?;

    // Render home page.
    // Sort posts in reverse "date" field order (should be mostly sorted already,
    // since we've walked the directory in reverse file creation date.
    posts.sort_by(|a, b| b.date().cmp(a.date()));
    let index_context = HashMap::from([("posts", &posts)]);
    let rendered = tera().render("index.html", &tera::Context::from_serialize(index_context)?)?;

    let index_path = WEBSITE_DIR.join("index.html");
    let mut index_file = File::create(&index_path)?;
    index_file.write_all(rendered.as_bytes())?;

    println!("WRITE {}", index_path.as_os_str().to_string_lossy());

    // load_syntax_theme("gruvbox (Light) (Hard)")?;

    Ok(())
}
