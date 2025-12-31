use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct PageFrontMatter {
    title: Box<str>,
    date: Box<str>,
    slug: Option<Box<str>>,
    #[serde(default)]
    draft: bool,
    #[serde(flatten)]
    all_else: HashMap<Box<str>, Box<str>>,
}

impl PageFrontMatter {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn date(&self) -> &str {
        &self.date
    }

    pub fn slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    pub fn draft(&self) -> bool {
        self.draft
    }

    pub fn all_else(&self) -> &HashMap<Box<str>, Box<str>> {
        &self.all_else
    }
}

#[derive(Debug, Serialize)]
pub struct FrontPageInfo {
    title: Box<str>,
    date: Box<str>,
    slug: Box<str>,
}

impl FrontPageInfo {
    pub fn new(
        title: impl Into<Box<str>>,
        date: impl Into<Box<str>>,
        slug: impl Into<Box<str>>,
    ) -> Self {
        Self {
            title: title.into(),
            date: date.into(),
            slug: slug.into(),
        }
    }

    #[allow(dead_code)]
    pub fn title(&self) -> &str {
        self.title.as_ref()
    }

    pub fn date(&self) -> &str {
        self.date.as_ref()
    }

    #[allow(dead_code)]
    pub fn slug(&self) -> &str {
        self.slug.as_ref()
    }

    pub fn to_map(&self) -> HashMap<&str, &str> {
        HashMap::from([
            ("title", self.title.as_ref()),
            ("date", self.date.as_ref()),
            ("slug", self.slug.as_ref()),
        ])
    }
}
