//! Lazy background highlighting: parsing never runs in the editor's layouter.
use crate::{
    markdown_syntax::{MarkdownHighlighter, Span, Token},
    themes,
};
use eframe::egui::{
    self, Color32, FontId, Stroke, TextFormat,
    text::{Galley, LayoutJob},
};
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
    thread,
};

const MAX_HIGHLIGHT_BYTES: usize = 1024 * 1024;

struct Request {
    id: u64,
    revision: u64,
    source: Arc<str>,
    ctx: egui::Context,
}

struct Highlighted {
    id: u64,
    revision: u64,
    spans: Result<Vec<Span>, String>,
}

struct Worker {
    requests: mpsc::Sender<Request>,
    results: mpsc::Receiver<Highlighted>,
}

struct Cached {
    source: Arc<str>,
    revision: u64,
    spans: Vec<Span>,
    layout: Option<LayoutJob>,
    theme: String,
    font_size: f32,
    line_height: f32,
    foreground: Color32,
}

#[derive(Default)]
pub struct Syntax {
    worker: Option<Worker>,
    disabled: bool,
    revision: u64,
    documents: HashMap<u64, Cached>,
}

impl Syntax {
    pub fn remove(&mut self, id: u64) {
        self.documents.remove(&id);
    }

    fn submit(&mut self, request: Request) {
        if self.disabled {
            return;
        }
        if self.worker.is_none() {
            let (tx, rx) = mpsc::channel();
            let (results_tx, results_rx) = mpsc::channel();
            match thread::Builder::new()
                .name("mde-syntax".into())
                .spawn(move || run_worker(rx, results_tx))
            {
                Ok(_) => {
                    self.worker = Some(Worker {
                        requests: tx,
                        results: results_rx,
                    })
                }
                Err(error) => {
                    self.disabled = true;
                    eprintln!("mde: cannot start syntax highlighting: {error}");
                    return;
                }
            }
        }
        if self
            .worker
            .as_ref()
            .unwrap()
            .requests
            .send(request)
            .is_err()
        {
            self.disabled = true;
            self.worker = None;
        }
    }

    fn poll(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        while let Ok(result) = worker.results.try_recv() {
            let Some(cached) = self.documents.get_mut(&result.id) else {
                continue;
            };
            // Never apply results for text already changed or a document already closed.
            if cached.revision != result.revision {
                continue;
            }
            match result.spans {
                Ok(spans) => {
                    cached.spans = spans;
                    cached.layout = None;
                }
                Err(error) => eprintln!("mde: syntax highlighting: {error}"),
            }
        }
    }

    pub fn layout(
        &mut self,
        ui: &egui::Ui,
        id: u64,
        text: &str,
        theme: &str,
        font_size: f32,
        wrap_width: f32,
    ) -> Arc<Galley> {
        self.poll();
        let font = FontId::monospace(font_size);
        let line_height =
            ui.fonts_mut(|fonts| fonts.row_height(&font)) + ui.spacing().extra_text_line_spacing;
        if text.len() > MAX_HIGHLIGHT_BYTES {
            self.documents.remove(&id);
            let job = make_layout(
                text,
                &[],
                theme,
                font_size,
                line_height,
                ui.visuals().text_color(),
                wrap_width,
            );
            return ui.fonts_mut(|fonts| fonts.layout_job(job));
        }
        if self
            .documents
            .get(&id)
            .is_none_or(|cached| cached.source.as_ref() != text)
        {
            self.revision += 1;
            let source: Arc<str> = Arc::from(text);
            let mut spans = self.documents.get(&id).map_or_else(Vec::new, |cached| {
                rebase(&cached.source, text, &cached.spans)
            });
            // Empty documents require neither a parser nor a worker thread.
            if text.is_empty() {
                spans.clear();
            }
            self.documents.insert(
                id,
                Cached {
                    source: source.clone(),
                    revision: self.revision,
                    spans,
                    layout: None,
                    theme: String::new(),
                    font_size: 0.0,
                    line_height: 0.0,
                    foreground: Color32::TRANSPARENT,
                },
            );
            if !text.is_empty() {
                self.submit(Request {
                    id,
                    revision: self.revision,
                    source,
                    ctx: ui.ctx().clone(),
                });
            }
        }
        let cached = self.documents.get_mut(&id).unwrap();
        let foreground = ui.visuals().text_color();
        if cached.layout.is_none()
            || cached.theme != theme
            || cached.font_size != font_size
            || cached.line_height != line_height
            || cached.foreground != foreground
        {
            cached.layout = Some(make_layout(
                text,
                &cached.spans,
                theme,
                font_size,
                line_height,
                foreground,
                wrap_width,
            ));
            cached.theme = theme.to_owned();
            cached.font_size = font_size;
            cached.line_height = line_height;
            cached.foreground = foreground;
        }
        let mut job = cached.layout.as_ref().unwrap().clone();
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    }
}

fn run_worker(requests: mpsc::Receiver<Request>, results: mpsc::Sender<Highlighted>) {
    let mut highlighter = match MarkdownHighlighter::new() {
        Ok(highlighter) => highlighter,
        Err(error) => {
            eprintln!("mde: cannot initialize Tree-sitter: {error}");
            return;
        }
    };
    while let Ok(request) = requests.recv() {
        // Keep only the newest queued version of each document. Switching tabs
        // must not discard a pending job belonging to a different document.
        let mut pending = HashMap::from([(request.id, request)]);
        while let Ok(request) = requests.try_recv() {
            pending.insert(request.id, request);
        }
        let mut pending: Vec<_> = pending.into_values().collect();
        pending.sort_by_key(|request| std::cmp::Reverse(request.revision));
        for request in pending {
            let spans = highlighter.highlight(&request.source);
            if results
                .send(Highlighted {
                    id: request.id,
                    revision: request.revision,
                    spans,
                })
                .is_err()
            {
                return;
            }
            request.ctx.request_repaint();
        }
    }
}

fn make_layout(
    text: &str,
    spans: &[Span],
    theme: &str,
    font_size: f32,
    line_height: f32,
    foreground: Color32,
    wrap_width: f32,
) -> LayoutJob {
    let colors = themes::syntax_colors(theme);
    let base = TextFormat {
        font_id: FontId::monospace(font_size),
        color: foreground,
        line_height: Some(line_height),
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.keep_trailing_whitespace = true;
    let mut cursor = 0;
    for span in spans {
        if span.start < cursor || span.end > text.len() || text.get(span.start..span.end).is_none()
        {
            continue;
        }
        job.append(&text[cursor..span.start], 0.0, base.clone());
        let mut format = base.clone();
        format.color = match span.token {
            Token::Heading => colors.heading,
            Token::Marker => colors.marker,
            Token::Link => colors.link,
            Token::Code => colors.code,
            Token::Strong | Token::Emphasis => colors.emphasis,
            Token::Strike | Token::Comment => colors.comment,
            Token::Keyword => colors.keyword,
            Token::String => colors.string,
            Token::Number => colors.number,
            Token::Function => colors.function,
            Token::Type => colors.type_name,
            Token::Property => colors.property,
            Token::Operator => colors.operator,
            Token::Variable => colors.variable,
        };
        format.italics = matches!(span.token, Token::Emphasis | Token::Comment);
        if span.token == Token::Strike {
            format.strikethrough = Stroke::new(1.0, format.color);
        }
        if span.token == Token::Link {
            format.underline = Stroke::new(1.0, format.color);
        }
        job.append(&text[span.start..span.end], 0.0, format);
        cursor = span.end;
    }
    job.append(&text[cursor..], 0.0, base);
    job
}

// Keep colors of the unchanged prefix and suffix while a fresh parse is pending.
// UTF-8 boundaries are rounded down; spans overlapping the edit are truncated.
fn rebase(old: &str, new: &str, spans: &[Span]) -> Vec<Span> {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = old[prefix..]
        .bytes()
        .rev()
        .zip(new[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    let mut rebased = Vec::new();
    for span in spans {
        if span.start < prefix {
            let end = span.end.min(prefix);
            if span.start < end {
                rebased.push(Span {
                    start: span.start,
                    end,
                    token: span.token,
                });
            }
        }
        if span.end > old_end {
            let start = span.start.max(old_end) - old_end + new_end;
            let end = span.end - old_end + new_end;
            if start < end {
                rebased.push(Span {
                    start,
                    end,
                    token: span.token,
                });
            }
        }
    }
    rebased
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_is_lazy_and_unchanged_text_does_not_enqueue_more_work() {
        use std::time::{Duration, Instant};
        let ctx = egui::Context::default();
        let mut syntax = Syntax::default();
        let frame = |syntax: &mut Syntax, text, theme, width| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                syntax.layout(ui, 1, text, theme, 16.0, width);
            });
            output.textures_delta.clear();
        };
        frame(&mut syntax, "", "dracula", 500.0);
        assert!(syntax.worker.is_none());
        frame(&mut syntax, "# Cached heading", "dracula", 500.0);
        assert!(syntax.worker.is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while syntax.documents[&1].spans.is_empty() && Instant::now() < deadline {
            syntax.poll();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!syntax.documents[&1].spans.is_empty());
        let revision = syntax.revision;
        for (theme, width) in [("dracula", 500.0), ("dracula", 300.0), ("nord", 300.0)] {
            frame(&mut syntax, "# Cached heading", theme, width);
            assert_eq!(syntax.revision, revision);
        }
        frame(&mut syntax, "# Changed heading", "nord", 300.0);
        assert_eq!(syntax.revision, revision + 1);
    }

    #[test]
    fn unchanged_spans_survive_unicode_edits_without_invalid_ranges() {
        let old = "# Привет 🦀\n`код`";
        let spans = vec![
            Span {
                start: 2,
                end: old.find('\n').unwrap(),
                token: Token::Heading,
            },
            Span {
                start: old.find('`').unwrap(),
                end: old.len(),
                token: Token::Code,
            },
        ];
        for new in [
            "# ПриXвет 🦀\n`код`",
            "# Привет 😃\n`код`",
            "# Привет 🦀\n`ко`",
            "",
            old,
        ] {
            let rebased = rebase(old, new, &spans);
            let mut end = 0;
            for span in rebased {
                assert!(span.start >= end);
                assert!(span.start < span.end);
                assert!(new.get(span.start..span.end).is_some());
                end = span.end;
            }
        }
        let inserted = "Вступление\n# Привет 🦀\n`код`";
        let rebased = rebase(old, inserted, &spans);
        assert_eq!(&inserted[rebased[0].start..rebased[0].end], "Привет 🦀");
    }

    #[test]
    fn layout_preserves_text_and_theme_changes_colors_without_parsing() {
        let text = "# Тема\nfn main()";
        let spans = [
            Span {
                start: 2,
                end: text.find('\n').unwrap(),
                token: Token::Heading,
            },
            Span {
                start: text.find("fn").unwrap(),
                end: text.find("fn").unwrap() + 2,
                token: Token::Keyword,
            },
        ];
        let make = |theme| make_layout(text, &spans, theme, 16.0, 20.0, Color32::WHITE, 500.0);
        let mocha = make("catppuccin-mocha");
        let gruvbox = make("gruvbox-light");
        assert_eq!(mocha.text, text);
        assert_eq!(gruvbox.text, text);
        assert!(
            mocha
                .sections
                .iter()
                .zip(&gruvbox.sections)
                .any(|(a, b)| a.format.color != b.format.color)
        );
        assert_eq!(
            usize::from(mocha.sections.last().unwrap().byte_range.end),
            text.len()
        );
    }

    #[test]
    fn stale_results_and_closed_documents_are_ignored() {
        let mut syntax = Syntax::default();
        let (tx, _) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        syntax.worker = Some(Worker {
            requests: tx,
            results: rx,
        });
        syntax.documents.insert(
            1,
            Cached {
                source: Arc::from("new"),
                revision: 2,
                spans: vec![],
                layout: None,
                theme: String::new(),
                font_size: 0.0,
                line_height: 0.0,
                foreground: Color32::WHITE,
            },
        );
        let span = Span {
            start: 0,
            end: 3,
            token: Token::Heading,
        };
        results
            .send(Highlighted {
                id: 1,
                revision: 1,
                spans: Ok(vec![span.clone()]),
            })
            .unwrap();
        syntax.poll();
        assert!(syntax.documents[&1].spans.is_empty());
        results
            .send(Highlighted {
                id: 1,
                revision: 2,
                spans: Ok(vec![span]),
            })
            .unwrap();
        syntax.poll();
        assert_eq!(syntax.documents[&1].spans.len(), 1);
        syntax.remove(1);
        results
            .send(Highlighted {
                id: 1,
                revision: 2,
                spans: Ok(vec![]),
            })
            .unwrap();
        syntax.poll();
        assert!(syntax.documents.is_empty());
    }
}
