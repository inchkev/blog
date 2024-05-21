use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File},
    io::Write,
    path::Path,
    sync::OnceLock,
};

use anyhow::Result;
use gray_matter::{engine::YAML, Matter};
use kuchikiki::traits::*;
use lazy_static::lazy_static;
use serde::Deserialize;
use syntect::{
    html::{ClassStyle, ClassedHTMLGenerator},
    util::LinesWithEndings,
};
use tera::Tera;
use walkdir::WalkDir;

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
        tera.autoescape_on(vec![]);
        tera
    })
}

fn ss() -> &'static syntect::parsing::SyntaxSet {
    static PS: OnceLock<syntect::parsing::SyntaxSet> = OnceLock::new();
    PS.get_or_init(syntect::parsing::SyntaxSet::load_defaults_newlines)
}

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

fn get_image_dims<P: AsRef<Path>>(path: P) -> Result<imagesize::ImageSize> {
    let size = imagesize::size(path)?;
    Ok(size)
}

fn copy_media_and_update_source<P: AsRef<Path>>(html: &str, move_dir: P) -> String {
    let document = kuchikiki::parse_html().one(html);

    for img_tag in document.select("img").unwrap() {
        let img_src = {
            let attributes = img_tag.attributes.borrow();
            attributes.get("src").unwrap_or_default().to_owned()
        };

        let img_path = CONTENT_DIR.join(&img_src);
        let img_dest = move_dir.as_ref().join(&img_src);

        fs::copy(img_path, img_dest).unwrap();
        // let image = VipsImage::new_from_file_access(
        //     &img_path.to_string_lossy(),
        //     libvips::ops::Access::Random,
        //     false,
        // );

        let mut attributes_mut = img_tag.attributes.borrow_mut();
        // attributes_mut.insert("srcset", img_src.to_owned());
        // attributes_mut.insert("sizes", img_src.to_owned());
        if let Ok(img_dims) = get_image_dims(CONTENT_DIR.join(&img_src)) {
            attributes_mut.insert("width", img_dims.width.to_string());
            attributes_mut.insert("height", img_dims.height.to_string());
        }
    }
    document.to_string()
}

fn html_syntax_highlight(html: &str) -> String {
    let document = kuchikiki::parse_html().one(html);

    for code_tag in document.select("pre code").unwrap() {
        let Some(class) = ({
            let attributes = code_tag.attributes.borrow();
            attributes.get("class").map(|s| s.to_owned())
        }) else {
            continue;
        };

        let Some(language) = class.split_once('-').map(|p| p.1.to_owned()) else {
            continue;
        };

        let code = code_tag.text_contents();
        // dbg!(&language);

        let syntax = ss()
            .find_syntax_by_token(&language)
            .unwrap_or_else(|| ss().find_syntax_plain_text());

        let mut html_generator =
            ClassedHTMLGenerator::new_with_class_style(syntax, ss(), ClassStyle::Spaced);
        for line in LinesWithEndings::from(&code) {
            html_generator
                .parse_html_for_line_which_includes_newline(line)
                .unwrap();
        }

        let output_html = html_generator.finalize();
        let snippet = kuchikiki::parse_html().one(output_html);

        let node = code_tag.as_node().first_child().unwrap();
        if let Some(text) = node.as_text() {
            "".clone_into(&mut text.borrow_mut());
        }
        node.insert_after(snippet);
    }
    document.to_string()
}

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
        if entry.path().is_file() && entry.path().extension() == Some(OsStr::new("md")) {
            let content = fs::read_to_string(entry.path())?;

            let yaml_matter = Matter::<YAML>::new();
            let result = yaml_matter.parse(&content);
            let front_matter = result.data.unwrap().deserialize::<FrontMatter>()?;

            if front_matter.draft {
                continue;
            }

            let contents = result.content;

            let mut options = markdown::Options::gfm();
            options.compile.allow_dangerous_html = true;
            let html_contents = markdown::to_html_with_options(&contents, &options).unwrap();

            // copy images
            // let html_contents = enhance_media_and_update_html(&html_contents);

            let slug = front_matter
                .slug
                .unwrap_or_else(|| get_slug_from_path(entry.path()));

            // create directory for page
            let page_dir = WEBSITE_DIR.join(&slug);
            if page_dir.try_exists().is_ok_and(|exists| !exists) {
                fs::create_dir(WEBSITE_DIR.join(&slug)).unwrap();
            }

            // copy images
            let html_contents = copy_media_and_update_source(&html_contents, &page_dir);
            let html_contents = html_syntax_highlight(&html_contents);

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
