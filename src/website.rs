use std::cell::OnceCell;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::Result;
use glob::glob;
use gray_matter::ParsedEntity;
use kuchikiki::traits::TendrilSink;
use markdown::{CompileOptions, ParseOptions};
use regex::Regex;
use tera::{Context, Tera};

use crate::checksum::Checksum;
use crate::config::Config;
use crate::html;
use crate::page::{Page, PageFrontMatter};
use crate::page_bundle::PageBundle;
use crate::shortcode::ShortcodeManager;
use crate::state::StateManager;
use crate::FULL_REBUILD_GLOBS;

// Default directory/file names
const CONTENT_DIR: &str = "content";
const TEMPLATE_DIR: &str = "templates";
const STATIC_DIR: &str = "static";
const OUTPUT_DIR: &str = "website";
const THEME_DIR: &str = "themes";
const STATE_FILE: &str = "state.json";

fn yaml_matter() -> &'static gray_matter::Matter<gray_matter::engine::YAML> {
    use gray_matter::engine::YAML;
    use gray_matter::Matter;
    static MATTER: LazyLock<Matter<YAML>> = LazyLock::new(Matter::<YAML>::new);
    &MATTER
}

fn create_tera<P: AsRef<Path>>(template_path: P) -> Tera {
    let mut tera = Tera::new(&template_path.as_ref().join("*.html").to_string_lossy())
        .unwrap_or_else(|e| panic!("{e}"));
    tera.autoescape_on(vec![]);
    // TODO: register custom filters.
    // See https://keats.github.io/tera/docs/#built-in-filters
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

fn markdown_to_body_html(markdown: &str, options: &markdown::Options) -> (String, bool) {
    let html = markdown::to_html_with_options(markdown, options).unwrap();
    let html_document = kuchikiki::parse_html().one(html);
    let has_code_blocks = html::has_code_blocks(&html_document);
    if has_code_blocks {
        html::syntax_highlight_code_blocks(&html_document);
    }

    // Get just what's inside the <body> tag
    let body_content = html::get_body_children_of_document(&html_document)
        .map(|nr| nr.to_string())
        .collect();
    (body_content, has_code_blocks)
}

/// - copy referenced images to output directory
/// - add dimensions to images
/// - wraps images with figure tags
/// - updates references section
///
/// Returns (processed_html, copied_images).
fn postprocess_html<P: AsRef<Path>, Q: AsRef<Path>, R: AsRef<Path>>(
    html: String,
    page_dir: P,
    content_dir: Q,
    static_dir: R,
) -> Result<(String, Vec<OsString>)> {
    // Find body content boundaries to avoid parsing the full document
    // Exclude leading/trailing whitespace from parsing since kuchikiki/html5ever
    //drops them
    let body_start_re = Regex::new(r"<body[^>]*>\s*").unwrap();
    let body_end_re = Regex::new(r"\s*</body>").unwrap();

    let Some(start_match) = body_start_re.find(&html) else {
        return Ok((html, vec![]));
    };
    let Some(end_match) = body_end_re.find(&html[start_match.end()..]) else {
        return Ok((html, vec![]));
    };
    let start = start_match.end();
    let end = start + end_match.start();

    // Parse the body
    let body_content = kuchikiki::parse_html().one(&html[start..end]);
    html::add_dimensions_to_images(&body_content, &content_dir, static_dir);
    // TODO: this should really be done around markdown parsing time instead
    html::wrap_images_with_figure_tags(&body_content);
    let copied_images =
        html::copy_relative_path_images_and_update_image_src(&body_content, content_dir, page_dir)?;
    html::update_references_section(&body_content);

    // Re-serialize the body
    let new_body: String = html::get_body_children_of_document(&body_content)
        .map(|nr| nr.to_string())
        .collect();

    // Re-join
    let rejoined_html = format!("{}{}{}", &html[..start], new_body, &html[end..]);
    Ok((rejoined_html, copied_images))
}

pub struct Website {
    #[allow(dead_code)]
    base_path: PathBuf,
    content_path: PathBuf,
    template_path: PathBuf,
    static_path: PathBuf,
    output_path: PathBuf,
    #[allow(dead_code)]
    theme_path: PathBuf,
    config: Config,
    state_manager: StateManager,
    shortcode_manager: OnceCell<ShortcodeManager>,
    markdown_options: markdown::Options,
    tera: OnceCell<Tera>,
}

impl Website {
    pub fn init<P: AsRef<Path>, Q: AsRef<Path>>(path: P, config_path: Q) -> Result<Self> {
        let base_path = path.as_ref().to_path_buf();
        let state_path = base_path.join(STATE_FILE);
        let content_path = base_path.join(CONTENT_DIR);
        let template_path = base_path.join(TEMPLATE_DIR);
        let static_path = base_path.join(STATIC_DIR);
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
                ..CompileOptions::default()
            },
        };

        Ok(Self {
            base_path,
            content_path,
            template_path,
            static_path,
            output_path,
            theme_path,
            config,
            state_manager,
            shortcode_manager: OnceCell::new(),
            markdown_options,
            tera: OnceCell::new(),
        })
    }

    fn tera(&self) -> &Tera {
        self.tera.get_or_init(|| create_tera(&self.template_path))
    }

    fn shortcode_manager(&self) -> &ShortcodeManager {
        self.shortcode_manager
            .get_or_init(|| ShortcodeManager::new(&self.template_path))
    }

    pub fn bake(&mut self) -> Result<()> {
        // Ensure output directory exists
        fs::create_dir_all(&self.output_path)?;

        let mut page_bundle = PageBundle::default();

        // Check if any files that can trigger a full rebuild have changed
        let full_rebuild_paths: Vec<PathBuf> = FULL_REBUILD_GLOBS
            .iter()
            .flat_map(|pattern| {
                glob(pattern.as_ref())
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.ok().filter(|p| p.is_file()))
            })
            .collect();
        let should_full_rebuild = self
            .state_manager
            .fast_set_next_bulk_and_check_if_changed(full_rebuild_paths)?;
        if should_full_rebuild {
            println!("Full-rebuild files changed, regenerating all pages...");
        }

        // Collect markdown files and sort by creation time (newest first)
        let mut content_paths: Vec<_> =
            glob(self.content_path.join("*.md").to_string_lossy().as_ref())
                .unwrap()
                .into_iter()
                .filter_map(std::result::Result::ok)
                .collect();
        content_paths.sort_by_key(|p| Reverse(p.metadata().ok().and_then(|m| m.created().ok())));

        for path in content_paths {
            let file_contents = match fs::read_to_string(&path) {
                Ok(contents) => contents,
                Err(e) => {
                    eprintln!("cannot read {}:\n{e}", path.as_os_str().to_string_lossy());
                    continue;
                }
            };
            let parsed_file: ParsedEntity = match yaml_matter().parse(&file_contents) {
                Ok(pf) => pf,
                Err(e) => {
                    eprintln!("cannot parse {}:\n{e}", path.as_os_str().to_string_lossy());
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
                        "cannot parse front matter in {}:\n{e}",
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

            // Create new page directory, or collect existing files for cleanup later
            let page_dir = self.output_path.join(page_slug);
            let existing_files: HashSet<OsString> = match fs::read_dir(&page_dir) {
                Ok(entries) => entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.file_name())
                    .filter(|name| name != "index.html")
                    .collect(),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&page_dir)?;
                    HashSet::new()
                }
                Err(e) => {
                    eprintln!("cannot read page directory {}:\n{e}", page_dir.display());
                    continue;
                }
            };

            // Markdown -> HTML pipeline:
            let markdown = parsed_file.content;
            // 1. Process shortcodes in markdown.
            let Ok(markdown) = self.shortcode_manager().render_shortcodes(markdown) else {
                eprintln!(
                    "cannot render shortcodes in {}",
                    path.as_os_str().to_string_lossy()
                );
                self.state_manager.unset_checksum(&page_slug);
                continue;
            };
            // 2. Convert processed markdown to HTML. Because `markdown-rs`
            // outputs a full HTML page, we grab only its <body> content before
            // passing it to Tera.
            let (body_contents, has_code_blocks) =
                markdown_to_body_html(&markdown, &self.markdown_options);
            // Set some page metadata.
            page.set_content(body_contents)
                .set_has_code_block(has_code_blocks);
            // 3. Render ("bake") the HTML contents into a full-formed page.
            let rendered_page = page.parbake(self.tera())?;
            // 4. Perform some post-processing on the HTML page.
            let (rendered_page, copied_images) = match postprocess_html(
                rendered_page,
                &page_dir,
                &self.content_path,
                &self.static_path,
            ) {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("cannot postprocess {}:\n{e}", page.slug());
                    continue;
                }
            };
            // 4. Create the index.html file!
            let output_path = page_dir.join("index.html");
            let mut output_file = File::create(&output_path)?;
            output_file.write_all(rendered_page.as_bytes())?;

            println!("  WRITE {}", output_path.as_os_str().to_string_lossy());

            // Delete leftover (stale) files in the page directory.
            for stale_file in existing_files.difference(&copied_images.into_iter().collect()) {
                let stale_path = page_dir.join(stale_file);
                if let Err(e) = fs::remove_file(&stale_path) {
                    eprintln!("cannot delete stale file {}:\n{e}", stale_path.display());
                } else {
                    println!("  DELETE {}", stale_path.display());
                }
            }

            page_bundle.add_page(page);
        }

        // Delete stale page directories
        for slug in &self.state_manager.get_slugs_to_delete() {
            let delete_path = self.output_path.join(slug);
            if let Err(e) = fs::remove_dir_all(&delete_path) {
                eprintln!(
                    "cannot delete dir {}/:\n{e}",
                    delete_path.as_os_str().to_string_lossy()
                );
                continue;
            }
            println!("DELETE {}/", delete_path.as_os_str().to_string_lossy());
        }

        // Build home page (index).
        page_bundle.sort_pages();
        let index_checksum = Checksum::from_data(&serde_json::to_string(&page_bundle.pages())?);
        self.state_manager.set_index_checksum(index_checksum);
        if self.state_manager.should_rebuild_index() {
            let rendered_index = page_bundle.parbake(self.tera())?;
            let (rendered_index, _) = postprocess_html(
                rendered_index,
                &self.output_path,
                &self.content_path,
                &self.static_path,
            )?;

            let index_path = self.output_path.join("index.html");
            let mut index_file = File::create(&index_path)?;
            index_file.write_all(rendered_index.as_bytes())?;

            println!("WRITE {}", index_path.as_os_str().to_string_lossy());
        }

        if should_full_rebuild {
            // Build 404 page
            self.bake_404()?;
        }

        // Copy files in static directory
        self.copy_static_files()?;

        // Save new state file
        println!(
            "pretty print state cache: {}",
            self.config.pretty_print_state_cache
        );
        self.state_manager
            .write_state_file_and_commit(self.config.pretty_print_state_cache)?;

        // load_syntax_theme("gruvbox (Light) (Hard)", &self.theme_path, &self.output_path)?;

        Ok(())
    }

    fn bake_404(&self) -> Result<()> {
        let rendered = match self.tera().render("404.html", &Context::new()) {
            Ok(r) => r,
            Err(e) => {
                if matches!(e.kind, tera::ErrorKind::TemplateNotFound(_)) {
                    println!("cannot find template '404.html', consider adding one");
                    return Ok(());
                }
                return Err(e.into());
            }
        };
        let path = self.output_path.join("404.html");
        fs::write(&path, rendered)?;
        println!("WRITE {}", path.display());
        Ok(())
    }

    /// Copies files from the static directory to the output directory.
    ///
    /// - preserves directory structure
    /// - only copies directories and regular files
    /// - only copies files that have changed
    /// - overwrites existing files
    /// - continues on individual file errors
    /// - deletes stale files (deleted) and directories (empty)
    fn copy_static_files(&mut self) -> Result<()> {
        if !self.static_path.exists() {
            return Ok(());
        }
        let Ok(metadata) = fs::symlink_metadata(&self.static_path) else {
            return Ok(());
        };
        if !metadata.is_dir() {
            return Ok(());
        }
        let output_path = self.output_path.clone();
        self._copy_static_dir_recursive(Path::new(""), &output_path)?;

        // delete stale static files
        let stale_files = self
            .state_manager
            .get_stale_static_files_in_order_of_deletion();
        for (stale_file, is_file) in stale_files {
            let stale_path = output_path.join(stale_file);
            if is_file {
                if let Err(e) = fs::remove_file(&stale_path) {
                    eprintln!("cannot delete stale file {}:\n{e}", stale_file.display());
                } else {
                    println!("DELETE [static file] {}", stale_file.display());
                }
            } else {
                // empty directory (unless theres a page with the same name)
                // try to delete the directory
                if fs::remove_dir(&stale_path).is_ok() {
                    println!("DELETE [static dir ] {}/", stale_file.display());
                }
            }
        }

        Ok(())
    }

    /// Recursively copies contents of `src` directory to `dest` directory.
    ///
    /// - `root_path`: root static directory to copy from
    /// - `relative_path`: source path, relative to `static_dir`
    /// - `dest`: destination directory to copy to
    fn _copy_static_dir_recursive(&mut self, src_relative_path: &Path, dest: &Path) -> Result<()> {
        let dir_path = self.static_path.join(src_relative_path);
        let entries = match fs::read_dir(&dir_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("cannot read dir {}: {e}", dir_path.display());
                return Ok(());
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("cannot read direntry in {}: {e}", dir_path.display());
                    continue;
                }
            };
            let entry_path = entry.path();
            let file_type = match entry.file_type() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("cannot get filetype of {}: {e}", entry_path.display());
                    continue;
                }
            };

            // don't copy symlinks
            if file_type.is_symlink() {
                eprintln!("skipping symlink: {}", entry_path.display());
                continue;
            }

            let entry_file_name = entry.file_name();
            let dest_path = dest.join(&entry_file_name);

            let entry_relative_path = src_relative_path.join(entry_file_name);

            if file_type.is_file() {
                // Copy regular file (overwrites existing)
                let has_changed = self
                    .state_manager
                    .fast_set_next_static_file_state_and_check_if_changed(
                        &entry_path,
                        entry_relative_path,
                    )?;
                if !has_changed {
                    continue;
                }
                match fs::copy(&entry_path, &dest_path) {
                    Ok(_) => {
                        println!("COPY [static file] {}", dest_path.display());
                    }
                    Err(e) => {
                        eprintln!(
                            "cannot copy {} to {}: {e}",
                            entry_path.display(),
                            dest_path.display()
                        );
                    }
                }
            } else if file_type.is_dir() {
                // Create destination directory if needed
                let does_not_exist = self
                    .state_manager
                    .fast_set_next_static_file_state_and_check_if_changed(
                        &entry_path,
                        entry_relative_path.clone(),
                    )?;
                if does_not_exist {
                    // NOTE: a directory could be marked as removed according
                    // to its updated state in the static map, but it could
                    // still correctly exist if there was a page with the same
                    // name that created that directory.
                    println!("COPY [static dir ] {}/", dest_path.display());
                    if let Err(e) = fs::create_dir_all(&dest_path) {
                        eprintln!("cannot create dir {}: {e}", dest_path.display());
                        continue;
                    }
                }
                // Recurse into subdirectory
                self._copy_static_dir_recursive(&entry_relative_path, &dest_path)?;
            }
        }

        Ok(())
    }
}
