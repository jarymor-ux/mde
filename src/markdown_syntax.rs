//! Tree-sitter highlighting for Markdown and the languages embedded in code fences.
//!
//! Configurations and parser allocations are reused. The highlight crate reparses a
//! changed document; callers should cache results until the document changes.

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Heading,
    Marker,
    Link,
    Code,
    Strong,
    Emphasis,
    Strike,
    Comment,
    Keyword,
    String,
    Number,
    Function,
    Type,
    Property,
    Operator,
    Variable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub token: Token,
}

// These names are deliberately shared by every configuration: Highlight indices
// refer to this list, rather than the order of captures in a grammar's query.
// More specific names override their generic dot-separated parent captures.
const CAPTURES: &[(&str, Token)] = &[
    ("text.title", Token::Heading),
    ("text.literal", Token::Code),
    ("text.strong", Token::Strong),
    ("text.emphasis", Token::Emphasis),
    ("text.strike", Token::Strike),
    ("text.uri", Token::Link),
    ("text.reference", Token::Link),
    ("punctuation", Token::Marker),
    ("string.escape", Token::String),
    ("comment", Token::Comment),
    ("keyword", Token::Keyword),
    ("constant", Token::Number),
    ("constant.builtin", Token::Keyword),
    ("boolean", Token::Keyword),
    ("number", Token::Number),
    ("float", Token::Number),
    ("string", Token::String),
    ("character", Token::String),
    ("function", Token::Function),
    ("constructor", Token::Function),
    ("type", Token::Type),
    ("attribute", Token::Property),
    ("property", Token::Property),
    ("tag", Token::Property),
    ("operator", Token::Operator),
    ("variable", Token::Variable),
    ("module", Token::Type),
    ("label", Token::Property),
];

pub struct MarkdownHighlighter {
    highlighter: Highlighter,
    markdown: HighlightConfiguration,
    inline: HighlightConfiguration,
    rust: HighlightConfiguration,
    javascript: HighlightConfiguration,
    python: HighlightConfiguration,
    json: HighlightConfiguration,
    bash: HighlightConfiguration,
}

impl MarkdownHighlighter {
    pub fn new() -> Result<Self, String> {
        let names: Vec<_> = CAPTURES.iter().map(|(name, _)| *name).collect();
        let config = |language, name: &str, highlights: &str, injections: &str, locals: &str| {
            let mut config =
                HighlightConfiguration::new(language, name, highlights, injections, locals)
                    .map_err(|error| format!("{name} highlight query: {error}"))?;
            config.configure(&names);
            Ok::<_, String>(config)
        };
        // Preserve the enclosing literal style for an unsupported code fence.
        // `none` is intentionally not configured, so it does not reset that style.
        // Markdown's inline and fence nodes contain anonymous punctuation
        // children. Include them so the injected parser sees the complete source.
        let markdown_injections = r#"
(fenced_code_block
  (info_string (language) @injection.language)
  (code_fence_content) @injection.content
  (#set! injection.include-children))
((inline) @injection.content
  (#set! injection.language "markdown_inline")
  (#set! injection.include-children))
"#;
        let inline_query = format!(
            "{}\n(strikethrough) @text.strike\n",
            tree_sitter_md::HIGHLIGHT_QUERY_INLINE
        );
        // Rust's bundled query groups numeric and boolean literals under
        // constant.builtin. Give numbers their own role in the editor palette.
        let rust_query = tree_sitter_rust::HIGHLIGHTS_QUERY
            .replace(
                "(integer_literal) @constant.builtin",
                "(integer_literal) @number",
            )
            .replace(
                "(float_literal) @constant.builtin",
                "(float_literal) @number",
            );
        let javascript_query = format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
        );
        Ok(Self {
            highlighter: Highlighter::new(),
            markdown: config(
                tree_sitter_md::LANGUAGE.into(),
                "markdown",
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
                markdown_injections,
                "",
            )?,
            inline: config(
                tree_sitter_md::INLINE_LANGUAGE.into(),
                "markdown_inline",
                &inline_query,
                tree_sitter_md::INJECTION_QUERY_INLINE,
                "",
            )?,
            rust: config(
                tree_sitter_rust::LANGUAGE.into(),
                "rust",
                &rust_query,
                tree_sitter_rust::INJECTIONS_QUERY,
                "",
            )?,
            javascript: config(
                tree_sitter_javascript::LANGUAGE.into(),
                "javascript",
                &javascript_query,
                tree_sitter_javascript::INJECTIONS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            )?,
            python: config(
                tree_sitter_python::LANGUAGE.into(),
                "python",
                tree_sitter_python::HIGHLIGHTS_QUERY,
                "",
                "",
            )?,
            json: config(
                tree_sitter_json::LANGUAGE.into(),
                "json",
                tree_sitter_json::HIGHLIGHTS_QUERY,
                "",
                "",
            )?,
            bash: config(
                tree_sitter_bash::LANGUAGE.into(),
                "bash",
                tree_sitter_bash::HIGHLIGHT_QUERY,
                "",
                "",
            )?,
        })
    }

    pub fn highlight(&mut self, text: &str) -> Result<Vec<Span>, String> {
        let Self {
            highlighter,
            markdown,
            inline,
            rust,
            javascript,
            python,
            json,
            bash,
        } = self;
        let events = highlighter
            .highlight(markdown, text.as_bytes(), None, None, |name| {
                match name.to_ascii_lowercase().as_str() {
                    "markdown_inline" => Some(&*inline),
                    "rust" | "rs" => Some(&*rust),
                    "javascript" | "js" | "jsx" | "node" => Some(&*javascript),
                    "python" | "py" => Some(&*python),
                    "json" => Some(&*json),
                    "bash" | "sh" | "shell" | "zsh" => Some(&*bash),
                    _ => None,
                }
            })
            .map_err(|error| error.to_string())?;
        let mut stack = Vec::new();
        let mut spans: Vec<Span> = Vec::new();
        for event in events {
            match event.map_err(|error| error.to_string())? {
                HighlightEvent::HighlightStart(highlight) => {
                    let token = CAPTURES
                        .get(highlight.0)
                        .ok_or_else(|| "Unknown highlight capture index".to_owned())?
                        .1;
                    stack.push(token);
                }
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                }
                HighlightEvent::Source { start, end } => {
                    if start >= end {
                        continue;
                    }
                    if end > text.len()
                        || !text.is_char_boundary(start)
                        || !text.is_char_boundary(end)
                    {
                        return Err("Highlight contains an invalid UTF-8 range".to_owned());
                    }
                    let Some(&token) = stack.last() else {
                        continue;
                    };
                    if let Some(last) = spans.last_mut() {
                        if last.end > start {
                            return Err("Highlight ranges overlap".to_owned());
                        }
                        if last.end == start && last.token == token {
                            last.end = end;
                            continue;
                        }
                    }
                    spans.push(Span { start, end, token });
                }
            }
        }
        Ok(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(spans: &[Span], text: &str, needle: &str, token: Token) {
        let start = text.find(needle).expect("fixture contains expected text");
        let end = start + needle.len();
        assert!(
            spans
                .iter()
                .any(|span| span.start <= start && span.end >= end && span.token == token),
            "{needle:?} should be {token:?}; spans: {spans:?}"
        );
    }

    fn valid(spans: &[Span], text: &str) {
        let mut previous_end = 0;
        for span in spans {
            assert!(span.start >= previous_end);
            assert!(span.start < span.end);
            assert!(text.get(span.start..span.end).is_some());
            previous_end = span.end;
        }
    }

    #[test]
    fn markdown_styles_have_correct_unicode_byte_ranges() {
        let text =
            "# Привет 🦀\n\n*курсив* **жирный** ~~убран~~ [ссылка](https://example.com) `код`\n";
        let spans = MarkdownHighlighter::new().unwrap().highlight(text).unwrap();
        valid(&spans, text);
        covers(&spans, text, "Привет 🦀", Token::Heading);
        covers(&spans, text, "курсив", Token::Emphasis);
        covers(&spans, text, "жирный", Token::Strong);
        covers(&spans, text, "убран", Token::Strike);
        covers(&spans, text, "ссылка", Token::Link);
        covers(&spans, text, "https://example.com", Token::Link);
        covers(&spans, text, "код", Token::Code);
    }

    #[test]
    fn nested_inline_styles_restore_the_enclosing_heading() {
        let text = "# Начало **жирный** `код` конец\n";
        let spans = MarkdownHighlighter::new().unwrap().highlight(text).unwrap();
        valid(&spans, text);
        covers(&spans, text, "Начало", Token::Heading);
        covers(&spans, text, "жирный", Token::Strong);
        covers(&spans, text, "код", Token::Code);
        covers(&spans, text, "конец", Token::Heading);
    }

    #[test]
    fn rust_fence_is_a_real_language_injection() {
        let text = "До 🦀\n\n```rust\nfn main() {\n    // привет\n    let x = \"мир\";\n    println!(\"{}\", 42);\n}\n```\n";
        let spans = MarkdownHighlighter::new().unwrap().highlight(text).unwrap();
        valid(&spans, text);
        covers(&spans, text, "fn", Token::Keyword);
        covers(&spans, text, "main", Token::Function);
        covers(&spans, text, "// привет", Token::Comment);
        covers(&spans, text, "мир", Token::String);
        covers(&spans, text, "42", Token::Number);
    }

    #[test]
    fn selected_fence_languages_and_aliases_are_highlighted() {
        let mut highlighter = MarkdownHighlighter::new().unwrap();
        for (language, code, keyword, string, number) in [
            (
                "js",
                "const item = \"hello\"; const n = 42;",
                "const",
                "hello",
                "42",
            ),
            (
                "PY",
                "if True:\n    item = \"hello\"\n    n = 42",
                "if",
                "hello",
                "42",
            ),
            (
                "json",
                "{\"item\": \"hello\", \"n\": 42, \"yes\": true}",
                "true",
                "hello",
                "42",
            ),
            (
                "sh",
                "if true; then echo \"hello\"; exit 42; fi",
                "if",
                "hello",
                "",
            ),
        ] {
            let text = format!("```{language}\n{code}\n```\n");
            let spans = highlighter.highlight(&text).unwrap();
            valid(&spans, &text);
            covers(&spans, &text, keyword, Token::Keyword);
            covers(&spans, &text, string, Token::String);
            // The Bash grammar has no number highlight capture: numeric command
            // arguments remain ordinary shell text, as in its official query.
            if !number.is_empty() {
                covers(&spans, &text, number, Token::Number);
            }
        }
    }

    #[test]
    fn unknown_fence_remains_readable_as_code() {
        let text = "```unknown-language\nтекст 🦀 + 123\n```\n";
        let spans = MarkdownHighlighter::new().unwrap().highlight(text).unwrap();
        valid(&spans, text);
        covers(&spans, text, "текст 🦀 + 123", Token::Code);
    }

    #[test]
    fn incomplete_edits_always_produce_valid_ranges() {
        let text = "# Заголовок 🦀\n\n**жирный [ссылка](https://example.com)\n\n```rust\nfn main() { let x = \"незакрытая";
        let mut highlighter = MarkdownHighlighter::new().unwrap();
        for end in (0..=text.len()).filter(|&end| text.is_char_boundary(end)) {
            let prefix = &text[..end];
            valid(&highlighter.highlight(prefix).unwrap(), prefix);
        }
        // Reuse the parser after shrinking and completely replacing its input.
        for text in ["", "plain", "*текст*", "```python\nif", "# 🦀", ""] {
            valid(&highlighter.highlight(text).unwrap(), text);
        }
    }
}
