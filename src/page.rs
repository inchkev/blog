use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tera::{Context, Tera};

fn try_get_slug_from_path<P: AsRef<Path>>(path: P) -> Option<String> {
    let stem = path.as_ref().file_stem()?.to_str()?;
    let (_date, slug) = stem.split_once('_')?;
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_owned())
    }
}

#[derive(Debug, Deserialize)]
pub struct PageFrontMatter {
    title: Option<Box<str>>,
    #[serde(default)]
    date: Box<str>,
    slug: Option<Box<str>>,
    #[serde(default)]
    draft: bool,
    #[serde(flatten)]
    extra: HashMap<Box<str>, Box<str>>,
}

impl PageFrontMatter {
    pub fn slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    pub fn draft(&self) -> bool {
        self.draft
    }
}

#[derive(Debug, Serialize)]
pub struct Page {
    slug: Box<str>,
    content: Option<Box<str>>,
    title: Option<Box<str>>,
    date: Box<str>,
    has_code_block: bool,
    draft: bool,
    extra: HashMap<Box<str>, Box<str>>,
}

impl Page {
    pub fn try_from_front_matter<P: AsRef<Path>>(
        front_matter: PageFrontMatter,
        file_path: P,
    ) -> Result<Self> {
        let slug = {
            if let Some(slug) = front_matter.slug() {
                slug.to_owned()
            } else {
                try_get_slug_from_path(file_path).ok_or(anyhow!("No slug found"))?
            }
        };
        Ok(Self {
            slug: slug.into(),
            content: None,
            title: front_matter.title,
            date: front_matter.date,
            has_code_block: false,
            draft: front_matter.draft,
            extra: front_matter.extra,
        })
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn set_content(&mut self, content: impl Into<Box<str>>) -> &mut Self {
        self.content = Some(content.into());
        self
    }

    pub fn set_has_code_block(&mut self, has_code_block: bool) -> &mut Self {
        self.has_code_block = has_code_block;
        self
    }

    pub fn parbake(&self, tera: &Tera) -> Result<String> {
        let mut context = Context::new();
        context.insert("content", &self.content);
        context.insert("slug", &self.slug);
        context.insert("title", &self.title.as_deref().map(tera::escape_html));
        context.insert("date", &self.date);
        context.insert("has_code_block", &self.has_code_block);
        context.insert("draft", &self.draft);
        for (k, v) in &self.extra {
            context.insert(k.as_ref(), &tera::escape_html(v.as_ref()));
        }
        Ok(tera.render("page.html", &context)?)
    }
}

impl From<Page> for PartialPage {
    fn from(page: Page) -> Self {
        Self {
            slug: page.slug,
            title: page.title.as_deref().map(|t| tera::escape_html(t).into()),
            date: page.date,
            draft: page.draft,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PartialPage {
    slug: Box<str>,
    title: Option<Box<str>>,
    date: Box<str>,
    draft: bool,
}

impl PartialPage {
    pub fn date(&self) -> &str {
        &self.date
    }
}
