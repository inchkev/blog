use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use glob::glob;
use regex::Regex;
use tera::{Context, Tera};

static SHORTCODE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{-?\s*[a-zA-Z_][a-zA-Z0-9_]*\s*\(").unwrap());

const SHORTCODE_DIR: &str = "_shortcodes";

struct Shortcode {
    template: String,
}

impl tera::Function for Shortcode {
    fn call(&self, args: &HashMap<String, tera::Value>) -> tera::Result<tera::Value> {
        let mut context = Context::new();
        for (k, v) in args {
            context.insert(k, v);
        }
        // Render using one_off - doesn't need a Tera instance
        let html = Tera::one_off(&self.template, &context, false)?;
        Ok(tera::Value::String(html))
    }
}

pub struct ShortcodeManager {
    tera: Option<RefCell<Tera>>,
}

impl ShortcodeManager {
    pub fn new<P: AsRef<Path>>(template_path: P) -> Self {
        let shortcode_dir_path = template_path.as_ref().join(SHORTCODE_DIR);

        let shortcode_paths: Vec<_> =
            glob(shortcode_dir_path.join("*.html").to_string_lossy().as_ref())
                .unwrap()
                .into_iter()
                .flatten()
                .collect();
        if shortcode_paths.is_empty() {
            return ShortcodeManager { tera: None };
        }

        let mut tera = Tera::default();
        let mut shortcode_names = Vec::new();

        for path in shortcode_paths {
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!("Error reading shortcode template {}: {}", path.display(), e);
                    continue;
                }
            };

            let name = path.file_stem().unwrap().to_string_lossy();
            // TODO: validate shortcode templates?
            let shortcode = Shortcode {
                template: content.trim().to_string(),
            };
            tera.register_function(&name, shortcode);
            shortcode_names.push(name.to_string());
        }

        println!("Registered shortcodes: {}", shortcode_names.join(", "));

        ShortcodeManager {
            tera: Some(RefCell::new(tera)),
        }
    }

    pub fn render_shortcodes(&self, content: String) -> tera::Result<String> {
        // No shortcodes templates defined
        let Some(ref tera) = self.tera else {
            return Ok(content);
        };
        // No shortcodes found in `content`
        if !SHORTCODE_REGEX.is_match(&content) {
            return Ok(content);
        }
        tera.borrow_mut().render_str(&content, &Context::new())
    }
}
