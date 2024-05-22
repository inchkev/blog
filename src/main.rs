use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::Path,
    sync::OnceLock,
};

use anyhow::Result;
use gray_matter::{engine::YAML, Matter};
use kuchikiki::traits::TendrilSink;
use lazy_static::lazy_static;
use serde::Deserialize;
use syntect::html::ClassStyle;
use tera::Tera;
use walkdir::WalkDir;

mod html;

lazy_static! {
    static ref CONTENT_DIR: &'static Path = Path::new("content");
    static ref TEMPLATE_DIR: &'static Path = Path::new("templates");
    static ref THEME_DIR: &'static Path = Path::new("themes");
    static ref WEBSITE_DIR: &'static Path = Path::new("website");
}

fn tera() -> &'static Tera {
    static TERA: OnceLock<Tera> = OnceLock::new();
    TERA.get_or_init(|| {
        let mut tera = Tera::new(&TEMPLATE_DIR.join("*.html").to_string_lossy()).unwrap();
        // don't autoescape anything
        tera.autoescape_on(vec![]);
        tera
    })
}

pub fn ss() -> &'static syntect::parsing::SyntaxSet {
    static PS: OnceLock<syntect::parsing::SyntaxSet> = OnceLock::new();
    PS.get_or_init(syntect::parsing::SyntaxSet::load_defaults_newlines)
}

#[allow(dead_code)]
fn ts() -> &'static syntect::highlighting::ThemeSet {
    static PS: OnceLock<syntect::highlighting::ThemeSet> = OnceLock::new();
    PS.get_or_init(|| syntect::highlighting::ThemeSet::load_from_folder(THEME_DIR.clone()).unwrap())
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct FrontMatter {
    title: String,
    date: String,
    slug: Option<String>,
    #[serde(default)]
    draft: bool,
}

fn process_html<P: AsRef<Path>>(html: &str, page_dir: P) -> String {
    let document = kuchikiki::parse_html().one(html);
    html::copy_media(&document, page_dir);
    html::syntax_highlight_code_blocks(&document);
    document.to_string()
}

#[allow(dead_code)]
fn load_syntax_theme(theme: &str) -> Result<()> {
    let theme = &ts().themes[theme];
    let css = syntect::html::css_for_theme_with_class_style(theme, ClassStyle::Spaced)?;

    let css_path = WEBSITE_DIR.join("syntax.css");
    let mut css_file = File::create(css_path)?;
    css_file.write_all(css.as_bytes())?;

    Ok(())
}

fn get_slug_from_path<P: AsRef<Path>>(path: P) -> String {
    path.as_ref()
        .file_stem()
        .and_then(|stem| stem.to_str()?.split_once('_').map(|x| x.1))
        .unwrap_or_default()
        .into()
}

fn main() -> Result<()> {
    let mut posts = Vec::new();

    for entry in WalkDir::new(*CONTENT_DIR)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() && entry.path().extension().is_some_and(|s| s == "md") {
            let file_contents = fs::read_to_string(entry.path())?;

            let yaml_matter = Matter::<YAML>::new();
            let result = yaml_matter.parse(&file_contents);
            let front_matter = result.data.unwrap().deserialize::<FrontMatter>()?;
            let contents = result.content;

            if front_matter.draft {
                continue;
            }

            let mut options = markdown::Options::gfm();
            options.compile.allow_dangerous_html = true;
            let html_contents = markdown::to_html_with_options(&contents, &options).unwrap();

            let slug = front_matter
                .slug
                .unwrap_or_else(|| get_slug_from_path(entry.path()));

            // create directory for page
            let page_dir = WEBSITE_DIR.join(&slug);
            if page_dir.try_exists().is_ok_and(|exists| !exists) {
                fs::create_dir(WEBSITE_DIR.join(&slug)).unwrap();
            }

            // - re-formats the generated html
            // - copies images to each page's directory
            let html_contents = process_html(&html_contents, &page_dir);

            let post_context = HashMap::from([
                ("title", front_matter.title.clone()),
                ("slug", slug.clone()),
                ("date", front_matter.date.clone()),
                ("contents", html_contents),
            ]);

            let rendered =
                tera().render("page.html", &tera::Context::from_serialize(&post_context)?)?;

            let output_path = page_dir.join("index.html");
            let mut output_file = File::create(output_path)?;
            output_file.write_all(rendered.as_bytes())?;

            posts.push(post_context);
        }
    }

    let index_context = HashMap::from([("posts", &posts)]);

    let rendered = tera().render("index.html", &tera::Context::from_serialize(index_context)?)?;

    let index_path = WEBSITE_DIR.join("index.html");
    let mut index_file = File::create(index_path)?;
    index_file.write_all(rendered.as_bytes())?;

    // load_syntax_theme("gruvbox (Light) (Hard)")?;

    Ok(())
}
