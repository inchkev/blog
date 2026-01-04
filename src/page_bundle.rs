use anyhow::Result;
use serde::Serialize;
use tera::{Context, Tera};

use crate::page::{Page, PartialPage};

#[derive(Debug, Serialize)]
pub struct PageBundle {
    pages: Vec<PartialPage>,
}

impl Default for PageBundle {
    fn default() -> Self {
        Self { pages: Vec::new() }
    }
}

impl PageBundle {
    pub fn pages(&self) -> &[PartialPage] {
        &self.pages
    }

    pub fn add_page(&mut self, page: Page) -> &mut Self {
        self.pages.push(page.into());
        self
    }

    pub fn sort_pages(&mut self) {
        // Sort pages in reverse "date" field order (should be mostly sorted already,
        // since we've walked the directory in reverse file creation date.
        self.pages.sort_by(|a, b| b.date().cmp(a.date()));
    }

    /// Note: to ensure a consistent order of pages, call `sort_pages` first.
    pub fn parbake(&self, tera: &Tera) -> Result<String> {
        let mut context = Context::new();
        context.insert("pages", &self.pages);
        Ok(tera.render("index.html", &context)?)
    }
}
