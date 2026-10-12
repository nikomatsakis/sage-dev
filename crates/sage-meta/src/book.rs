//! Minimal mdBook protocol adapter for our specification anchor role.
use std::ffi::OsString;
use std::io::{self, Read};

pub fn preprocess(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    if args.first().is_some_and(|arg| arg == "supports") && args.len() == 2 {
        return if args[1] == "html" || args[1] == "linkcheck2" {
            Ok(())
        } else {
            Err("specification anchors support HTML and linkcheck2 renderers".into())
        };
    }
    if !args.is_empty() {
        return Err("usage: sage-meta preprocess [supports html]".into());
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let (_, mut book): (serde_json::Value, serde_json::Value) = serde_json::from_str(&input)?;
    // mdBook 0.5 renamed `sections` to `items`; keep the 0.4 Pages build usable.
    let items_key = if book.get("items").is_some() {
        "items"
    } else {
        "sections"
    };
    let sections = book
        .get_mut(items_key)
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("mdBook input must contain book items or sections")?;
    render_sections(sections)?;
    println!("{}", serde_json::to_string(&book)?);
    Ok(())
}

fn render_sections(sections: &mut [serde_json::Value]) -> Result<(), Box<dyn std::error::Error>> {
    for section in sections {
        let Some(chapter) = section.get_mut("Chapter") else {
            continue;
        };
        let content = chapter
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or("mdBook chapter has no content")?;
        let rendered = sage_meta::spec::render_anchors(content).map_err(|mut errors| {
            let path = chapter
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<chapter>");
            for error in &mut errors {
                error.file = path.into();
            }
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        chapter["content"] = rendered.into();
        if let Some(children) = chapter
            .get_mut("sub_items")
            .and_then(serde_json::Value::as_array_mut)
        {
            render_sections(children)?;
        }
    }
    Ok(())
}
