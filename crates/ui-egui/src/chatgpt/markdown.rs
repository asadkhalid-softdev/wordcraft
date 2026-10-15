//! Native Markdown preview. Model-provided images never load local files or remote URLs.
use std::collections::BTreeMap;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::{WordApp, theme::Tokens};

pub(super) fn show(app: &mut WordApp, ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &str) {
    let safe = safe_markdown(&normalize_math(text));
    let mut equations = BTreeMap::new();
    for event in Parser::new_ext(&safe, Options::ENABLE_MATH) {
        let (source, inline) = match event {
            Event::InlineMath(source) => (source, true),
            Event::DisplayMath(source) => (source, false),
            _ => continue,
        };
        if source.len() > 4096 || equations.len() >= 32 {
            continue;
        }
        let nodes = wordcraft_doc::math_latex::parse_latex(&source);
        let props = wordcraft_doc::props::CharProps { size: Some(14.0), ..Default::default() };
        let resolved = app.session.doc.styles.resolve_char(None, &props);
        let layout = wordcraft_layout::math::layout_equation(
            &wordcraft_doc::math::Math { nodes: nodes.clone(), ..Default::default() },
            "",
            &resolved,
            !inline,
            ui.available_width().max(1.0),
            &Default::default(),
            &mut 0,
        );
        let width = (layout.width + 8.0).clamp(16.0, ui.available_width().clamp(16.0, 1200.0));
        let height = (layout.ascent + layout.descent + 8.0).clamp(24.0, 320.0);
        let size = egui::vec2(width, height);
        let key = format!("chatgpt:{inline}:{source}");
        let preview = crate::previews::math_texture(app, ui.ctx(), &key, &nodes, !inline, 14.0, size);
        equations.insert((source.to_string(), inline), (preview, size));
    }
    let math = move |ui: &mut egui::Ui, source: &str, inline: bool| match equations.get(&(source.to_owned(), inline)) {
        Some((Some((texture, uv)), size)) => {
            let response = ui.add(egui::Image::new((*texture, *size)).uv(*uv).tint(Tokens::get(ui.ctx()).text));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Equation"));
        }
        Some((None, _)) => {
            ui.spinner();
        }
        None => {
            ui.label("Equation exceeds preview limit");
        }
    };
    CommonMarkViewer::new().explicit_image_uri_scheme(true).render_math_fn(Some(&math)).show(ui, cache, &safe);
}

/// ChatGPT commonly uses LaTeX delimiters; leave code spans and fenced examples untouched.
pub(super) fn normalize_math(text: &str) -> String {
    let mut result = String::new();
    let mut copied = 0usize;
    for (event, range) in Parser::new(text).into_offset_iter() {
        if !matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))) || range.start < copied {
            continue;
        }
        if let Some(prefix) = text.get(copied..range.start) {
            result.push_str(&math_delimiters(prefix));
        }
        if let Some(code) = text.get(range.clone()) {
            result.push_str(code);
        }
        copied = range.end;
    }
    if let Some(rest) = text.get(copied..) {
        result.push_str(&math_delimiters(rest));
    }
    result
}

fn math_delimiters(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("\\(").into_iter().chain(rest.find("\\[")).min() {
        let Some(after_open) = rest.get(start.saturating_add(2)..) else { break };
        let display = rest.get(start..start.saturating_add(2)) == Some("\\[");
        let closing = if display { "\\]" } else { "\\)" };
        let Some(end) = after_open.find(closing) else {
            result.push_str(rest.get(..start.saturating_add(2)).unwrap_or(rest));
            rest = after_open;
            continue;
        };
        result.push_str(rest.get(..start).unwrap_or_default());
        let delimiter = if display { "$$" } else { "$" };
        result.push_str(delimiter);
        result.push_str(after_open.get(..end).unwrap_or_default());
        result.push_str(delimiter);
        rest = after_open.get(end.saturating_add(2)..).unwrap_or_default();
    }
    result.push_str(rest);
    result
}

/// Remove images and non-web hyperlinks before the viewer can ask egui to load or open them.
pub(super) fn safe_markdown(text: &str) -> String {
    let mut parser = Parser::new_ext(text, Options::all()).into_offset_iter();
    let mut out = String::new();
    let mut copied = 0usize;
    while let Some((event, range)) = parser.next() {
        let blocked =
            matches!(&event, Event::Start(Tag::Image { .. })) || matches!(&event, Event::Start(Tag::Link { dest_url, .. }) if !safe_link(dest_url));
        if !blocked {
            continue;
        }
        if let Some(prefix) = text.get(copied..range.start) {
            out.push_str(prefix);
        }
        let mut depth = 1usize;
        let mut end = range.end;
        for (event, range) in parser.by_ref() {
            end = end.max(range.end);
            match event {
                Event::Start(_) => depth = depth.saturating_add(1),
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                Event::Text(t) | Event::Code(t) => {
                    for ch in t.chars() {
                        if "\\`*_{}[]()<>!#$|".contains(ch) {
                            out.push('\\');
                        }
                        out.push(ch);
                    }
                }
                _ => {}
            }
        }
        copied = end;
    }
    if let Some(rest) = text.get(copied..) {
        out.push_str(rest);
    }
    out
}

fn safe_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https://", "http://", "mailto:"].iter().any(|prefix| lower.starts_with(prefix)) && !url.chars().any(char::is_control)
}
