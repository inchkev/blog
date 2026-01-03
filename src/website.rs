use std::cell::OnceCell;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::Result;
use glob::glob;
use gray_matter::ParsedEntity;
use kuchikiki::traits::TendrilSink;
use markdown::{CompileOptions, ParseOptions};

use crate::checksum::Checksum;
use crate::config::Config;
use crate::html;
use crate::state::StateManager;
use crate::types::{FrontPageInfo, PageFrontMatter};
use crate::FULL_REBUILD_GLOBS;

// Default directory/file names
const CONTENT_DIR: &str = "content";
const TEMPLATE_DIR: &str = "templates";
const THEME_DIR: &str = "themes";
const OUTPUT_DIR: &str = "website";
const STATE_FILE: &str = "state.json";

// todos:
// - parallelize article processing
// - bufwriter?

fn yaml_matter() -> &'static gray_matter::Matter<gray_matter::engine::YAML> {
    use gray_matter::engine::YAML;
    use gray_matter::Matter;
    static MATTER: LazyLock<Matter<YAML>> = LazyLock::new(Matter::<YAML>::new);
    &MATTER
}

fn create_tera<P: AsRef<Path>>(template_path: P) -> tera::Tera {
    let mut tera = tera::Tera::new(&template_path.as_ref().join("*.html").to_string_lossy())
        .unwrap_or_else(|e| panic!("{e}"));
    tera.autoescape_on(vec![]);
    tera
}

fn ts<P: AsRef<Path>>(theme_path: P) -> syntect::highlighting::ThemeSet {
    syntect::highlighting::ThemeSet::load_from_folder(theme_path).unwrap_or_else(|e| panic!("{e}"))
}

#[allow(dead_code)]
fn load_syntax_theme<P: AsRef<Path>>(theme: &str, theme_path: P, output_path: P) -> Result<()> {
    let theme_set = ts(theme_path);
    let theme = &theme_set.themes[theme];
    let css = syntect::html::css_for_theme_with_class_style(theme, html::SYNTECT_CLASSSTYLE)?;

    let css_path = output_path.as_ref().join("syntax.css");
    let mut css_file = File::create(css_path)?;
    css_file.write_all(css.as_bytes())?;

    Ok(())
}

pub struct Website {
    #[allow(dead_code)]
    base_path: PathBuf,
    content_path: PathBuf,
    template_path: PathBuf,
    output_path: PathBuf,
    #[allow(dead_code)]
    theme_path: PathBuf,
    config: Config,
    state_manager: StateManager,
    markdown_options: markdown::Options,
    tera: OnceCell<tera::Tera>,
}

fn process_html<P: AsRef<Path>>(html: &str, content_dir: P, page_dir: P) -> Result<(String, bool)> {
    let document = kuchikiki::parse_html().one(html);

    html::copy_images_and_add_dimensions(&document, content_dir, page_dir)?;
    html::wrap_images_with_figure_tags(&document);
    let has_code_blocks = html::has_code_blocks(&document);
    if has_code_blocks {
        html::syntax_highlight_code_blocks(&document);
    }
    html::update_references_section(&document);

    Ok((html::finish(&document), has_code_blocks))
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

impl Website {
    pub fn init<P: AsRef<Path>, Q: AsRef<Path>>(path: P, config_path: Q) -> Result<Self> {
        let base_path = path.as_ref().to_path_buf();
        let state_path = base_path.join(STATE_FILE);
        let content_path = base_path.join(CONTENT_DIR);
        let template_path = base_path.join(TEMPLATE_DIR);
        let output_path = base_path.join(OUTPUT_DIR);
        let theme_path = base_path.join(THEME_DIR);

        let config = Config::from_file(config_path)?;
        let state_manager = StateManager::from_file(&state_path)?;

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

        Ok(Self {
            base_path,
            content_path,
            template_path,
            output_path,
            theme_path,
            config,
            state_manager,
            markdown_options,
            tera: OnceCell::new(),
        })
    }

    fn tera(&self) -> &tera::Tera {
        self.tera.get_or_init(|| create_tera(&self.template_path))
    }

    pub fn bake(&mut self) -> Result<()> {
        // Ensure output directory exists
        fs::create_dir_all(&self.output_path)?;

        let mut posts = Vec::<FrontPageInfo>::new();

        // Get checksum for files that can trigger a full rebuild
        let full_rebuild_checksum = Checksum::from_globs_par(FULL_REBUILD_GLOBS);
        if self
            .state_manager
            .set_full_rebuild_checksum(full_rebuild_checksum)
        {
            println!("Full-rebuild files changed, regenerating all pages...");
        }

        // Collect markdown files and sort by creation time (newest first)
        let mut content_paths: Vec<_> = glob(&format!("{}/*.md", self.content_path.display()))
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .collect();
        content_paths.sort_by_key(|p| Reverse(p.metadata().ok().and_then(|m| m.created().ok())));

        for path in content_paths {
            // print!("READ {} ... ", path.as_os_str().to_string_lossy());
            // stdout().flush()?;

            let file_contents = fs::read_to_string(&path)?;
            let parsed_file: ParsedEntity = yaml_matter().parse(&file_contents)?;
            let Some(front_matter_data) = parsed_file.data else {
                // println!("skipped (no data)");
                continue;
            };
            let front_matter = front_matter_data.deserialize::<PageFrontMatter>()?;
            if front_matter.draft() && !self.config.include_drafts {
                // println!("skipped (draft)");
                continue;
            }

            let slug = if let Some(s) = front_matter.slug() {
                s.to_string()
            } else {
                let Some(s) = try_get_slug_from_path(&path) else {
                    // println!("skipped (no slug)");
                    continue;
                };
                s
            };

            let front_page_info = FrontPageInfo::new(
                front_matter.title().unwrap_or(&slug),
                front_matter.date(),
                slug.clone(),
            );

            let file_checksum = Checksum::from_data(&file_contents);
            self.state_manager.set_checksum(slug.clone(), file_checksum);

            // Skip if a rebuild not needed
            if !self.state_manager.should_rebuild(&slug) {
                posts.push(front_page_info);
                // println!("skipped (no changes)");
                continue;
            }

            let html_contents =
                markdown::to_html_with_options(&parsed_file.content, &self.markdown_options)
                    .unwrap();

            // Create directory for page
            let page_dir = self.output_path.join(&slug);
            if let Ok(false) = page_dir.try_exists() {
                fs::create_dir(self.output_path.join(&slug)).unwrap();
            }

            let mut post_context = front_page_info.to_map();

            // - re-formats the generated html
            // - copies images to each page's directory
            // - and more. see function
            let (html_contents, has_code_blocks) =
                process_html(&html_contents, &self.content_path, &page_dir)?;

            post_context.insert("contents", html_contents.into());
            post_context.insert("hascodeblock", has_code_blocks.into());
            post_context.extend(
                front_matter
                    .all_else()
                    .iter()
                    .map(|(k, v)| (k.as_ref(), v.as_ref().into())),
            );

            // Render article page
            let rendered = self
                .tera()
                .render("page.html", &tera::Context::from_serialize(post_context)?)?;

            let output_path = page_dir.join("index.html");
            let mut output_file = File::create(&output_path)?;
            output_file.write_all(rendered.as_bytes())?;

            println!("  WRITE {}", output_path.as_os_str().to_string_lossy());

            posts.push(front_page_info);
        }

        // Delete stale files
        for slug in &self.state_manager.get_slugs_to_delete() {
            let delete_path = self.output_path.join(slug);
            println!("  DELETE {}/", delete_path.as_os_str().to_string_lossy());
            fs::remove_dir_all(delete_path)?;
        }

        // Sort posts in reverse "date" field order (should be mostly sorted already,
        // since we've walked the directory in reverse file creation date.
        posts.sort_by(|a, b| b.date().cmp(a.date()));

        // Build home page (index).
        let index_checksum = Checksum::from_data(&serde_json::to_string(&posts)?);
        self.state_manager.set_index_checksum(index_checksum);
        if self.state_manager.should_rebuild_index() {
            let index_context = HashMap::from([("posts", &posts)]);
            let rendered = self
                .tera()
                .render("index.html", &tera::Context::from_serialize(index_context)?)?;

            let index_path = self.output_path.join("index.html");
            let mut index_file = File::create(&index_path)?;
            index_file.write_all(rendered.as_bytes())?;

            println!("WRITE {}", index_path.as_os_str().to_string_lossy());
        }

        // Save new state file
        self.state_manager.write_state_file_and_commit()?;

        // load_syntax_theme("gruvbox (Light) (Hard)", &self.theme_path, &self.output_path)?;

        Ok(())
    }
}
