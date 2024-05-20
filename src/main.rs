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
use lazy_static::lazy_static;
use serde::Deserialize;
use tera::Tera;
use walkdir::WalkDir;

use html5ever::tree_builder::TreeSink;
use scraper::{Html, Selector};

lazy_static! {
    static ref CONTENT_DIR: &'static Path = Path::new("content");
    static ref TEMPLATE_DIR: &'static Path = Path::new("templates");
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

#[derive(Deserialize)]
#[allow(dead_code)]
struct FrontMatter {
    title: String,
    date: String,
    slug: Option<String>,
    #[serde(default)]
    draft: bool,
}

fn enhance_media_and_update_html(html: &str) -> String {
    let mut fragment = Html::parse_document(html);
    let image_selector = Selector::parse("img").unwrap();
    // fragment.

    for image_element in fragment.select(&image_selector) {
        let Some(src) = image_element.attr("src") else {
            continue;
        };
        // let a: TreeSink = fragment.into();

        // fragment.remove_from_parent(&image_element);
        let id = image_element.id();
        let mut tree = fragment.tree;
        let _ = tree.get_mut(id).unwrap().detach();

        let image_path = CONTENT_DIR.join(src);
        let destination_path = WEBSITE_DIR.join(src);
        let _ = fs::copy(image_path, destination_path);
    }
    fragment.html()
}

// fn test() {
//     let html = "<html><body>hello<p class=\"hello\">REMOVE ME</p></body></html>";
//     let selector = Selector::parse(".hello").unwrap();
//     let mut document = Html::parse_document(html);
//     let node_ids: Vec<_> = document.select(&selector).map(|x| x.id()).collect();
//     for id in node_ids {
//         document.remove_from_parent(&id);
//     }
//     assert_eq!(
//         document.html(),
//         "<html><head></head><body>hello</body></html>"
//     );
// }

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

            let html_contents =
                markdown::to_html_with_options(&contents, &markdown::Options::gfm()).unwrap();

            // copy images
            let html_contents = enhance_media_and_update_html(&html_contents);

            let slug = front_matter
                .slug
                .unwrap_or_else(|| get_slug_from_path(entry.path()));

            let post_context = HashMap::from([
                ("title", front_matter.title.clone()),
                ("slug", slug.clone()),
                ("date", front_matter.date.clone()),
                ("contents", html_contents),
            ]);

            let rendered =
                tera().render("page.html", &tera::Context::from_serialize(&post_context)?)?;

            let output_path = WEBSITE_DIR.join(format!("{}.html", slug));
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

    Ok(())
}
