//! Editable preview: keep inactive blocks rendered and expose only the active block.
use crate::{model::Document, syntax::Syntax};
use eframe::egui::{self, FontId, Key, KeyboardShortcut, Modifiers};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::{collections::HashMap, ops::Range, path::Path};

#[derive(Default)]
pub struct LivePreview {
    documents: HashMap<u64, PreviewDocument>,
}

#[derive(Default)]
struct PreviewDocument {
    source: String,
    blocks: Vec<Range<usize>>,
    references: String,
    active: Option<ActiveBlock>,
    generation: u64,
    undo: egui::util::undoer::Undoer<String>,
}

struct ActiveBlock {
    index: usize,
    range: Range<usize>,
    draft: String,
    focus: bool,
    click: Option<egui::Pos2>,
}

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
}

fn parse_blocks(text: &str) -> (Vec<Range<usize>>, String) {
    let parser = Parser::new_ext(text, options());
    let mut definitions: Vec<_> = parser
        .reference_definitions()
        .iter()
        .map(|(_, d)| d.span.clone())
        .collect();
    definitions.sort_by_key(|r| r.start);
    let mut references = String::new();
    for range in definitions {
        references.push('\n');
        references.push_str(&text[range]);
        references.push('\n');
    }
    let mut blocks = Vec::new();
    let mut footnotes = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    let mut end = 0;
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    start = range.start;
                    end = range.end;
                }
                if matches!(tag, Tag::FootnoteDefinition(_)) {
                    footnotes.push(range.clone());
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    blocks.push(start..end.max(range.end));
                }
            }
            Event::FootnoteReference(_) => footnotes.push(start..end),
            _ if depth == 0 => blocks.push(range),
            _ => {}
        }
    }
    // Footnotes need their definition context. Keep the connected region together
    // so references render once, exactly as in the whole-document viewer.
    if let (Some(first), Some(last)) = (
        footnotes.iter().map(|r| r.start).min(),
        footnotes.iter().map(|r| r.end).max(),
    ) {
        let begin = blocks.iter().position(|r| r.end > first).unwrap();
        let finish = blocks.iter().rposition(|r| r.start < last).unwrap();
        let merged = blocks[begin].start..blocks[finish].end;
        blocks.splice(begin..=finish, [merged]);
    }
    // Reference definitions and other non-rendered source remain editable.
    let mut complete = Vec::new();
    let mut cursor = 0;
    for range in blocks {
        if !text[cursor..range.start].trim().is_empty() {
            complete.push(cursor..range.start);
        }
        cursor = range.end;
        complete.push(range);
    }
    if !text[cursor..].trim().is_empty() {
        complete.push(cursor..text.len());
    }
    (complete, references)
}

impl PreviewDocument {
    fn synchronize(&mut self, text: &str) {
        if self.source != text {
            // A source-editor edit or replace invalidates the block's byte offsets.
            self.active = None;
            self.generation += 1;
            self.rebuild(text);
            // External edits create a new history branch too. add_undo alone
            // does not clear Undoer's redo stack.
            self.undo.feed_state(0.0, &self.source);
            self.undo.add_undo(&self.source);
        }
    }

    fn rebuild(&mut self, text: &str) {
        self.source = text.to_owned();
        (self.blocks, self.references) = parse_blocks(text);
    }

    fn finish(&mut self) {
        if self.active.take().is_some() {
            self.undo.add_undo(&self.source);
            (self.blocks, self.references) = parse_blocks(&self.source);
        }
    }

    fn activate(&mut self, index: usize) {
        self.finish();
        let Some(block) = self.blocks.get(index) else {
            return;
        };
        let mut range = block.clone();
        // Keep existing line endings outside TextEdit, including CRLF.
        while range.end > range.start
            && matches!(self.source.as_bytes()[range.end - 1], b'\n' | b'\r')
        {
            range.end -= 1;
        }
        self.generation += 1;
        self.undo.add_undo(&self.source);
        self.active = Some(ActiveBlock {
            index,
            draft: self.source[range.clone()].to_owned(),
            range,
            focus: true,
            click: None,
        });
    }

    fn apply_draft(&mut self, text: &mut String) {
        let active = self.active.as_mut().expect("active block");
        let old_end = active.range.end;
        let new_end = active.range.start + active.draft.len();
        text.replace_range(active.range.clone(), &active.draft);
        active.range.end = new_end;
        let delta = new_end as isize - old_end as isize;
        self.blocks[active.index].end = self.blocks[active.index]
            .end
            .checked_add_signed(delta)
            .unwrap();
        for range in &mut self.blocks[active.index + 1..] {
            range.start = range.start.checked_add_signed(delta).unwrap();
            range.end = range.end.checked_add_signed(delta).unwrap();
        }
        self.source.clone_from(text);
    }

    fn append(&mut self, text: &mut String) {
        self.finish();
        self.undo.add_undo(text);
        if !text.is_empty() {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            if !text.ends_with("\n\n") {
                text.push('\n');
            }
        }
        self.rebuild(text);
        let index = self.blocks.len();
        self.blocks.push(text.len()..text.len());
        self.generation += 1;
        self.active = Some(ActiveBlock {
            index,
            range: text.len()..text.len(),
            draft: String::new(),
            focus: true,
            click: None,
        });
    }
}

impl LivePreview {
    pub fn remove(&mut self, id: u64) {
        self.documents.remove(&id);
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        doc: &mut Document,
        cache: &mut CommonMarkCache,
        syntax: &mut Syntax,
        theme: &str,
        font_size: f32,
    ) {
        let state = self.documents.entry(doc.id).or_default();
        state.synchronize(&doc.text);
        let editor_id = egui::Id::new(("preview_editor", doc.id, state.generation));
        let owns_focus = ui.memory(|m| m.focused() == Some(editor_id));
        // A document-level history lets undo cross block boundaries.
        if owns_focus || !ui.ctx().egui_wants_keyboard_input() {
            // egui shortcut matching allows extra Shift: test redo first.
            let redo = ui.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers {
                        shift: true,
                        ..Modifiers::COMMAND
                    },
                    Key::Z,
                ))
            });
            let undo = !redo
                && ui.input_mut(|i| {
                    i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z))
                });
            let replacement = if undo {
                state.undo.undo(&doc.text).cloned()
            } else if redo {
                state.undo.redo(&doc.text).cloned()
            } else {
                None
            };
            if let Some(text) = replacement {
                doc.text = text;
                state.active = None;
                state.generation += 1;
                state.rebuild(&doc.text);
                ui.memory_mut(|m| m.surrender_focus(editor_id));
            }
        }
        let editor_id = egui::Id::new(("preview_editor", doc.id, state.generation));
        // egui releases keyboard focus on Escape before the UI pass.
        if state.active.is_some() && ui.input(|i| i.key_pressed(Key::Escape)) {
            state.finish();
            ui.memory_mut(|m| m.surrender_focus(editor_id));
        }
        if state.blocks.is_empty() && state.active.is_none() && doc.text.trim().is_empty() {
            state.blocks.push(doc.text.len()..doc.text.len());
            state.activate(0);
        }
        let directory = doc
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(Path::new("."));
        let mut activate = None;
        let mut editor_rect = None;
        for index in 0..state.blocks.len() {
            if state.active.as_ref().is_some_and(|a| a.index == index) {
                let active = state.active.as_mut().unwrap();
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
                    syntax.layout(
                        ui,
                        syntax_id(doc.id),
                        text.as_str(),
                        theme,
                        font_size,
                        width,
                    )
                };
                let mut output = egui::TextEdit::multiline(&mut active.draft)
                    .id(editor_id)
                    .font(FontId::monospace(font_size))
                    .layouter(&mut layouter)
                    .desired_width(ui.available_width())
                    .desired_rows(1)
                    .cursor_at_end(false)
                    .frame(egui::Frame::NONE)
                    .hint_text("Начните писать…")
                    .show(ui);
                editor_rect = Some(output.response.rect);
                if std::mem::take(&mut active.focus) {
                    output.response.request_focus();
                    if let Some(click) = active.click.take() {
                        let cursor = output.galley.cursor_from_pos(click - output.galley_pos);
                        let range = egui::text::CCursorRange::one(cursor);
                        output.state.cursor.set_char_range(Some(range));
                        output.state.clone().store(ui.ctx(), editor_id);
                        ui.ctx().request_repaint();
                    }
                }
                if output.response.changed() {
                    state.apply_draft(&mut doc.text);
                    ui.ctx().request_repaint();
                }
                if output.response.has_focus()
                    && let Some(range) = output.cursor_range
                {
                    let rect = output
                        .galley
                        .pos_from_cursor(range.primary)
                        .translate(output.galley_pos.to_vec2());
                    if ui.input(|i| {
                        i.key_pressed(Key::Enter)
                            || i.key_pressed(Key::ArrowDown)
                            || i.key_pressed(Key::ArrowUp)
                    }) {
                        ui.scroll_to_rect(rect, None);
                    }
                }
            } else {
                let range = state.blocks[index].clone();
                let fragment = format!("{}\n{}", &doc.text[range], state.references);
                let response = ui
                    .push_id(("rendered_block", doc.id, index), |ui| {
                        ui.style_mut().interaction.selectable_labels = false;
                        CommonMarkViewer::new()
                            .default_implicit_uri_scheme(format!("file://{}/", directory.display()))
                            .max_image_width(Some(ui.available_width().max(1.0) as usize))
                            .show(ui, cache, &fragment)
                    })
                    .inner
                    .response;
                let rect = response.rect.with_max_x(ui.max_rect().right());
                let hit = ui.interact(
                    rect,
                    egui::Id::new(("preview_hit", doc.id, index)),
                    egui::Sense::click(),
                );
                if hit.clicked() {
                    activate = Some(index);
                }
                if response.rect.height() < 2.0 {
                    let response = ui.add(
                        egui::Label::new(egui::RichText::new("Ссылки документа").small().weak())
                            .sense(egui::Sense::click()),
                    );
                    if response.clicked() {
                        activate = Some(index);
                    }
                }
            }
            ui.add_space(8.0);
        }
        if let Some(index) = activate {
            // finish() may reparse edited blocks, so locate the clicked block by byte offset.
            let start = state.blocks[index].start;
            state.finish();
            if let Some(index) = state
                .blocks
                .iter()
                .position(|r| r.start <= start && start < r.end)
            {
                state.activate(index);
                state.active.as_mut().unwrap().click = ui.input(|i| i.pointer.interact_pos());
                ui.ctx().request_repaint();
            }
        } else if let Some(rect) = editor_rect
            && ui.input(|i| {
                i.pointer.primary_clicked()
                    && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
            })
        {
            state.finish();
            ui.ctx().request_repaint();
        }
        let blank_height = (ui.clip_rect().bottom() - ui.cursor().min.y).max(48.0);
        let response = ui.allocate_response(
            egui::vec2(ui.available_width(), blank_height),
            egui::Sense::click(),
        );
        response.clone().on_hover_text("Новый абзац");
        if response.clicked() {
            state.append(&mut doc.text);
            ui.ctx().request_repaint();
        }
        state.undo.feed_state(ui.input(|i| i.time), &doc.text);
    }
}

pub fn syntax_id(document_id: u64) -> u64 {
    document_id | (1 << 63)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_blocks_keep_lists_tables_and_fences_whole() {
        let text = "# Заголовок\n\nАбзац **жирный**.\n\n- один\n- два\n\n```rust\nlet n = 1;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let (blocks, _) = parse_blocks(text);
        assert_eq!(blocks.len(), 5);
        assert!(text[blocks[2].clone()].contains("- два"));
        assert!(text[blocks[3].clone()].trim_end().ends_with("```"));
        assert!(text[blocks[4].clone()].contains("| 1 | 2 |"));
        for range in blocks {
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        }
    }

    #[test]
    fn unicode_edit_changes_only_active_block_and_keeps_crlf() {
        let mut text = "# Title\r\n\r\nПривет **мир**.\r\n\r\nПоследний.\r\n".to_owned();
        let mut state = PreviewDocument::default();
        state.synchronize(&text);
        state.activate(1);
        state.active.as_mut().unwrap().draft = "Привет **новый мир**!".to_owned();
        state.apply_draft(&mut text);
        assert_eq!(
            text,
            "# Title\r\n\r\nПривет **новый мир**!\r\n\r\nПоследний.\r\n"
        );
        assert_eq!(&text[state.blocks[2].clone()], "Последний.\r\n");
        state.finish();
        assert_eq!(state.blocks.len(), 3);
    }

    #[test]
    fn new_paragraphs_and_deletions_reparse_on_finish() {
        let mut text = "Один.\n\nДва.\n".to_owned();
        let mut state = PreviewDocument::default();
        state.synchronize(&text);
        state.activate(0);
        state.active.as_mut().unwrap().draft = "Один.\n\nНовый.".to_owned();
        state.apply_draft(&mut text);
        assert_eq!(state.blocks.len(), 2);
        state.finish();
        assert_eq!(state.blocks.len(), 3);
        state.activate(1);
        state.active.as_mut().unwrap().draft.clear();
        state.apply_draft(&mut text);
        state.finish();
        assert_eq!(state.blocks.len(), 2);
    }

    #[test]
    fn source_edit_invalidates_active_offsets_and_references_are_retained() {
        let mut state = PreviewDocument::default();
        state.synchronize("[ссылка][id]\n\n[id]: https://example.com\n");
        assert!(state.references.contains("[id]: https://example.com"));
        state.activate(0);
        state.synchronize("# Другой документ\n");
        assert!(state.active.is_none());
        assert!(state.references.is_empty());
    }

    #[test]
    fn footnotes_render_with_their_context_only_once() {
        let text = "До.\n\nТекст[^note].\n\nДругой абзац.\n\n[^note]: Сноска.\n\nПосле.\n";
        let (blocks, _) = parse_blocks(text);
        assert_eq!(blocks.len(), 3);
        let middle = &text[blocks[1].clone()];
        assert!(middle.contains("Текст[^note]"));
        assert!(middle.contains("[^note]: Сноска."));
        assert_eq!(
            Parser::new_ext(middle, options())
                .filter(|e| matches!(e, Event::FootnoteReference(_)))
                .count(),
            1
        );
    }

    #[test]
    fn opening_whitespace_and_finishing_without_edits_preserves_every_byte() {
        for text in [
            "",
            " \n\n",
            "# Heading\n\nParagraph\n\n",
            "> quote\n> nested\n\n- one\n  - two\n",
        ] {
            let mut state = PreviewDocument::default();
            state.synchronize(text);
            if state.blocks.is_empty() {
                state.blocks.push(text.len()..text.len());
            }
            for index in 0..state.blocks.len() {
                state.activate(index);
                state.finish();
                assert_eq!(state.source, text);
            }
        }
    }

    #[test]
    fn editing_after_undo_discards_redo_and_repeated_edits_keep_offsets() {
        let mut text = "First\n\nПоследний\n".to_owned();
        let mut state = PreviewDocument::default();
        state.synchronize(&text);
        state.activate(0);
        for draft in ["Расширенный первый абзац", "Короткий", "", "Первый"]
        {
            state.active.as_mut().unwrap().draft = draft.to_owned();
            state.apply_draft(&mut text);
            assert_eq!(&text[state.blocks[1].clone()], "Последний\n");
        }
        state.finish();
        text = state.undo.undo(&text).unwrap().clone();
        state.rebuild(&text);
        state.activate(0);
        state.active.as_mut().unwrap().draft = "Другая правка".to_owned();
        state.apply_draft(&mut text);
        state.undo.feed_state(1.0, &text);
        assert!(state.undo.redo(&text).is_none());
    }

    #[test]
    fn source_edit_after_undo_cannot_resurrect_previous_future() {
        let mut state = PreviewDocument::default();
        state.synchronize("Original");
        state.synchronize("Old future");
        let undone = state.undo.undo(&state.source).unwrap().clone();
        state.rebuild(&undone);
        state.synchronize("New source edit");
        assert!(state.undo.redo(&state.source).is_none());
    }

    #[test]
    fn escape_finishes_edit_even_after_egui_releases_focus() {
        let ctx = egui::Context::default();
        let mut doc = Document::blank(8);
        doc.text = "**Абзац**".to_owned();
        let mut preview = LivePreview::default();
        let state = preview.documents.entry(doc.id).or_default();
        state.synchronize(&doc.text);
        state.activate(0);
        let mut syntax = Syntax::default();
        let mut cache = CommonMarkCache::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            preview.show(
                ui,
                &mut doc,
                &mut cache,
                &mut syntax,
                "catppuccin-mocha",
                15.0,
            )
        });
        output.textures_delta.clear();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            preview.show(
                ui,
                &mut doc,
                &mut cache,
                &mut syntax,
                "catppuccin-mocha",
                15.0,
            )
        });
        output.textures_delta.clear();
        assert!(preview.documents[&doc.id].active.is_none());
        assert_eq!(doc.text, "**Абзац**");
    }

    #[test]
    fn actual_preview_keyboard_events_redo_instead_of_undo() {
        let ctx = egui::Context::default();
        let mut doc = Document::blank(7);
        doc.text = "One".to_owned();
        let mut preview = LivePreview::default();
        let state = preview.documents.entry(doc.id).or_default();
        state.synchronize(&doc.text);
        state.synchronize("Two");
        state.synchronize("Three");
        doc.text = state.undo.undo(&state.source).unwrap().clone();
        state.rebuild(&doc.text);
        state.activate(0);
        let id = egui::Id::new(("preview_editor", doc.id, state.generation));
        ctx.memory_mut(|m| m.request_focus(id));
        let modifiers = Modifiers {
            shift: true,
            ..Modifiers::COMMAND
        };
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        let mut syntax = Syntax::default();
        let mut cache = CommonMarkCache::default();
        let mut output = ctx.run_ui(input, |ui| {
            preview.show(
                ui,
                &mut doc,
                &mut cache,
                &mut syntax,
                "catppuccin-mocha",
                15.0,
            );
        });
        output.textures_delta.clear();
        assert_eq!(doc.text, "Three");
    }

    #[test]
    fn undo_redo_cross_block_boundaries() {
        let mut text = "Первый\n\nВторой\n".to_owned();
        let original = text.clone();
        let mut state = PreviewDocument::default();
        state.synchronize(&text);
        state.activate(0);
        state.active.as_mut().unwrap().draft.push('!');
        state.apply_draft(&mut text);
        state.finish();
        let first_edit = text.clone();
        state.activate(1);
        state.active.as_mut().unwrap().draft.push('?');
        state.apply_draft(&mut text);
        let both_edits = text.clone();
        text = state.undo.undo(&text).unwrap().clone();
        assert_eq!(text, first_edit);
        text = state.undo.undo(&text).unwrap().clone();
        assert_eq!(text, original);
        text = state.undo.redo(&text).unwrap().clone();
        assert_eq!(text, first_edit);
        text = state.undo.redo(&text).unwrap().clone();
        assert_eq!(text, both_edits);
    }
}
