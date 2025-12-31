use std::path::Path;

use anyhow::Result;
use kuchikiki::{iter::Siblings, traits::TendrilSink, NodeRef};
use syntect::{
    html::{ClassStyle, ClassedHTMLGenerator},
    util::LinesWithEndings,
};

use crate::{ss, CONTENT_DIR};

pub const SYNTECT_CLASSSTYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "_" };

fn get_image_dims<P: AsRef<Path>>(path: P) -> Result<imagesize::ImageSize> {
    let size = imagesize::size(path)?;
    Ok(size)
}

pub fn get_body_children_of_document(document: &NodeRef) -> Siblings {
    document.select_first("body").unwrap().as_node().children()
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
