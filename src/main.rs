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
use tera::Tera;
use walkdir::WalkDir;

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

fn get_image_dims<P: AsRef<Path>>(path: P) -> Result<imagesize::ImageSize> {
    let size = imagesize::size(path)?;
    Ok(size)
    // create directory for page
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

    Ok(())
}
