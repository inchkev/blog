use std::cell::OnceCell;
use std::cmp::Reverse;
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
use crate::page::{Page, PageFrontMatter};
use crate::page_bundle::PageBundle;
use crate::state::StateManager;
use crate::FULL_REBUILD_GLOBS;

// Default directory/file names
const CONTENT_DIR: &str = "content";
const TEMPLATE_DIR: &str = "templates";
const THEME_DIR: &str = "themes";
const OUTPUT_DIR: &str = "website";
const STATE_FILE: &str = "state.json";

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

        let mut page_bundle = PageBundle::default();

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

            let file_contents = match fs::read_to_string(&path) {
                Ok(contents) => contents,
                Err(e) => {
                    eprintln!("Error reading {}:\n{e}", path.as_os_str().to_string_lossy());
                    continue;
                }
            };
            let parsed_file: ParsedEntity = match yaml_matter().parse(&file_contents) {
                Ok(pf) => pf,
                Err(e) => {
                    eprintln!("Error parsing {}:\n{e}", path.as_os_str().to_string_lossy());
                    continue;
                }
            };
            let Some(front_matter_data) = parsed_file.data else {
                // println!("skipped (no data)");
                continue;
            };

            let page_front_matter = match front_matter_data.deserialize::<PageFrontMatter>() {
                Ok(fm) => fm,
                Err(e) => {
                    eprintln!(
                        "Error parsing front matter in {}:\n{e}",
                        path.as_os_str().to_string_lossy()
                    );
                    continue;
                }
            };
            if page_front_matter.draft() && !self.config.include_drafts {
                // println!("skipped (draft)");
                continue;
            }

            let Ok(mut page) = Page::try_from_front_matter(page_front_matter, &path) else {
                continue;
            };
            let page_slug = page.slug();

            let file_checksum = Checksum::from_data(&file_contents);
            self.state_manager
                .set_checksum(page_slug.to_string(), file_checksum);
            // Skip if page itself does not need to be rebuilt
            if !self.state_manager.should_rebuild(page_slug) {
                page_bundle.add_page(page);
                // println!("skipped (no changes)");
                continue;
            }

            // Never errors with normal markdown
            let html_contents =
                markdown::to_html_with_options(&parsed_file.content, &self.markdown_options)
                    .unwrap();

            // Create directory for page
            let page_dir = self.output_path.join(page_slug);
            if let Ok(false) = page_dir.try_exists() {
                fs::create_dir(&page_dir).unwrap();
            }

            // - re-formats the generated html
            // - copies images to each page's directory
            // - and more. see function
            let (html_contents, has_code_blocks) =
                process_html(&html_contents, &self.content_path, &page_dir)?;
            page.set_content(html_contents)
                .set_has_code_block(has_code_blocks);

            // Render article page
            let rendered_page = page.parbake(self.tera())?;

            let output_path = page_dir.join("index.html");
            let mut output_file = File::create(&output_path)?;
            output_file.write_all(rendered_page.as_bytes())?;

            println!("  WRITE {}", output_path.as_os_str().to_string_lossy());

            page_bundle.add_page(page);
        }

        // Delete stale files
        for slug in &self.state_manager.get_slugs_to_delete() {
            let delete_path = self.output_path.join(slug);
            if let Err(e) = fs::remove_dir_all(&delete_path) {
                eprintln!(
                    "Error deleting directory {}:\n{e}",
                    delete_path.as_os_str().to_string_lossy()
                );
                continue;
            }
            println!("  DELETE {}/", delete_path.as_os_str().to_string_lossy());
        }

        // Build home page (index).
        page_bundle.sort_pages();
        let index_checksum = Checksum::from_data(&serde_json::to_string(&page_bundle.pages())?);
        self.state_manager.set_index_checksum(index_checksum);
        if self.state_manager.should_rebuild_index() {
            let rendered_index = page_bundle.parbake(self.tera())?;

            let index_path = self.output_path.join("index.html");
            let mut index_file = File::create(&index_path)?;
            index_file.write_all(rendered_index.as_bytes())?;

            println!("WRITE {}", index_path.as_os_str().to_string_lossy());
        }

        // Save new state file
        self.state_manager.write_state_file_and_commit()?;

        // load_syntax_theme("gruvbox (Light) (Hard)", &self.theme_path, &self.output_path)?;

        Ok(())
    }
}
