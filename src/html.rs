use std::{path::Path, sync::LazyLock};

use anyhow::Result;
use kuchikiki::{iter::Siblings, traits::TendrilSink, NodeRef};
use syntect::{
    html::{ClassStyle, ClassedHTMLGenerator},
    util::LinesWithEndings,
};

use crate::CONTENT_DIR;

pub const SYNTECT_CLASSSTYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "_" };

#[must_use]
pub fn ss() -> &'static syntect::parsing::SyntaxSet {
    static PS: LazyLock<syntect::parsing::SyntaxSet> =
        LazyLock::new(syntect::parsing::SyntaxSet::load_defaults_newlines);
    &PS
}

fn get_image_dims<P: AsRef<Path>>(path: P) -> Result<imagesize::ImageSize> {
    let size = imagesize::size(path)?;
    Ok(size)
}

fn get_body_children_of_document(document: &NodeRef) -> Siblings {
    document.select_first("body").unwrap().as_node().children()
}

pub fn finish(document: &NodeRef) -> String {
    get_body_children_of_document(document)
        .map(|nr| nr.to_string())
        .collect()
}

pub fn copy_media_and_add_dimensions<P: AsRef<Path>>(document: &NodeRef, move_dir: P) {
    for img_tag in document.select("img").unwrap() {
        let img_src = {
            let attributes = img_tag.attributes.borrow();
            attributes.get("src").unwrap_or_default().to_owned()
        };

        let img_path = CONTENT_DIR.join(&img_src);
        let img_dest = move_dir.as_ref().join(&img_src);

        std::fs::copy(img_path, img_dest).unwrap();

        let mut attributes_mut = img_tag.attributes.borrow_mut();
        // attributes_mut.insert("srcset", img_src.to_owned());
        // attributes_mut.insert("sizes", img_src.to_owned());

        // add image width/height attributes (prevents layout shifts)
        if let Ok(img_dims) = get_image_dims(CONTENT_DIR.join(&img_src)) {
            attributes_mut.insert("width", img_dims.width.to_string());
            attributes_mut.insert("height", img_dims.height.to_string());
        }
    }
}

pub fn has_code_blocks(document: &NodeRef) -> bool {
    document.select("pre code").unwrap().next().is_some()
}

pub fn syntax_highlight_code_blocks(document: &NodeRef) {
    for code_tag in document.select("pre code").unwrap() {
        let Some(class) = ({
            let attributes = code_tag.attributes.borrow();
            attributes.get("class").map(ToOwned::to_owned)
        }) else {
            continue;
        };

        // generated class names take on the form "language-[LANG]"
        let Some(language) = class.split_once('-').map(|p| p.1.to_owned()) else {
            continue;
        };

        // dbg!(&language);

        let syntax = ss()
            .find_syntax_by_token(&language)
            .unwrap_or_else(|| ss().find_syntax_plain_text());

        let mut html_generator =
            ClassedHTMLGenerator::new_with_class_style(syntax, ss(), SYNTECT_CLASSSTYLE);

        let code = code_tag.text_contents();
        for line in LinesWithEndings::from(&code) {
            html_generator
                .parse_html_for_line_which_includes_newline(line)
                .unwrap();
        }

        let output_html = html_generator.finalize();
        let code_document = kuchikiki::parse_html().one(output_html);

        let node = code_tag.as_node().first_child().unwrap();
        // remove all existing text
        if let Some(text) = node.as_text() {
            text.borrow_mut().clear();
        }
        for code_node in get_body_children_of_document(&code_document) {
            node.insert_after(code_node);
        }
    }
}

pub fn update_references_section(document: &NodeRef) {
    for backref in document.select("a[data-footnote-backref]").unwrap() {
        let backref_node = backref.as_node();

        let backref_symbol_node = backref_node.first_child().unwrap();
        let mut backref_symbol_text = backref_symbol_node.as_text().unwrap().borrow_mut();
        *backref_symbol_text = backref_symbol_text.replace('\u{21A9}', "^");
        // NOTE: If you'd like to keep U+21A9, read the following:
        // Add the U+FE0F "text varation selector" character after
        // the backref symbol (U+21A9 leftwards arrow with hook)

        // Move backref to the beginning of the paragraph
        let Some(parent) = backref_node.parent() else {
            continue;
        };

        // Make sure parent is a <p> tag
        if parent.as_element().map(|e| e.name.local.as_ref()) != Some("p") {
            continue;
        }

        // Remove trailing space before backref if it exists
        if let Some(prev_sibling) = backref_node.previous_sibling() {
            if let Some(prev_text) = prev_sibling.as_text() {
                let mut prev_text = prev_text.borrow_mut();
                if prev_text.ends_with(' ') {
                    *prev_text = prev_text.trim_end().into();
                }
            }
        }

        // Move backref node to the beginning of the <p> tag
        backref_node.detach();
        if parent.first_child().is_some() {
            parent.prepend(backref_node.clone());
            // Add a space after the backref
            let space_node = NodeRef::new_text(" ");
            backref_node.insert_after(space_node);
        } else {
            // If paragraph is somehow empty, just append
            parent.append(backref_node.clone());
        }
    }
}
