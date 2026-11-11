use crate::model::{Document, FolderEntry, Settings, read_folder};
use eframe::egui::{self, FontId, Key, KeyboardShortcut, Modifiers, RichText};
use egui_commonmark::CommonMarkCache;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Instant,
};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Edit,
    Split,
    Preview,
}

#[derive(Clone, Copy)]
enum Pending {
    Close(u64),
    Quit,
}

enum Loaded {
    File(PathBuf, Result<Document, String>),
    Folder(PathBuf, Result<Vec<FolderEntry>, String>),
    Root(u64, PathBuf, PathBuf, Result<Vec<FolderEntry>, String>),
}

pub struct MdeApp {
    documents: Vec<Document>,
    active: usize,
    next_id: u64,
    view: View,
    settings: Settings,
    root: Option<PathBuf>,
    folders: HashMap<PathBuf, Vec<FolderEntry>>,
    expanded: HashSet<PathBuf>,
    loading: HashSet<PathBuf>,
    tx: mpsc::Sender<Loaded>,
    rx: mpsc::Receiver<Loaded>,
    initial_paths: Vec<PathBuf>,
    cache: CommonMarkCache,
    syntax: crate::syntax::Syntax,
    live_preview: crate::live_preview::LivePreview,
    autosave: crate::autosave::AutoSave,
    deferred_saves: Vec<(u64, bool)>,
    pending: Option<Pending>,
    error: Option<String>,
    notice: String,
    find_open: bool,
    query: String,
    replacement: String,
    find_offset: usize,
    find_selection: Option<egui::text::CCursorRange>,
    focus_editor: bool,
    first_frame: bool,
    started: Instant,
    allow_quit: bool,
    open_focus_id: Option<u64>,
    root_request: u64,
    large_preview_allowed: HashSet<u64>,
    commands_open: bool,
    project_open: bool,
}

impl MdeApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>, started: Instant) -> Self {
        Self::with_settings(&cc.egui_ctx, paths, started, Settings::load())
    }

    fn with_settings(
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        started: Instant,
        mut settings: Settings,
    ) -> Self {
        crate::fonts::install(ctx);
        settings.dark = crate::themes::apply(ctx, &settings.theme);
        let (tx, rx) = mpsc::channel();
        Self {
            documents: Vec::new(),
            active: 0,
            next_id: 1,
            view: View::Split,
            settings,
            root: None,
            folders: HashMap::new(),
            expanded: HashSet::new(),
            loading: HashSet::new(),
            tx,
            rx,
            initial_paths: paths,
            cache: CommonMarkCache::default(),
            syntax: crate::syntax::Syntax::default(),
            live_preview: crate::live_preview::LivePreview::default(),
            autosave: crate::autosave::AutoSave::default(),
            deferred_saves: Vec::new(),
            pending: None,
            error: None,
            notice: String::new(),
            find_open: false,
            query: String::new(),
            replacement: String::new(),
            find_offset: 0,
            find_selection: None,
            focus_editor: false,
            first_frame: true,
            started,
            allow_quit: false,
            open_focus_id: None,
            root_request: 0,
            large_preview_allowed: HashSet::new(),
            commands_open: false,
            project_open: false,
        }
    }

    fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        // Classification and canonicalization can hit a slow/network filesystem.
        // Keep both on the worker, including CLI startup paths.
        if let Some(index) = self
            .documents
            .iter()
            .position(|doc| doc.path.as_ref() == Some(&path))
        {
            self.active = index;
            self.open_focus_id = None;
            self.project_open = false;
        } else if self.loading.insert(path.clone()) {
            let id = self.next_id;
            self.next_id += 1;
            self.open_focus_id = Some(id);
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let resolved = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                let loaded = if resolved.is_dir() {
                    let result = read_folder(&resolved);
                    Loaded::Root(id, path, resolved, result)
                } else {
                    let result = Document::open(id, &resolved);
                    Loaded::File(path, result)
                };
                let _ = tx.send(loaded);
                ctx.request_repaint();
            });
        }
    }

    fn load_folder(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.folders.contains_key(&path) || !self.loading.insert(path.clone()) {
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = read_folder(&path);
            let _ = tx.send(Loaded::Folder(path, result));
            ctx.request_repaint();
        });
    }

    fn receive(&mut self) {
        while let Ok(loaded) = self.rx.try_recv() {
            match loaded {
                Loaded::File(path, result) => {
                    self.loading.remove(&path);
                    match result {
                        Ok(doc) => {
                            self.project_open = false;
                            if self.documents.iter().all(|other| other.path != doc.path) {
                                self.settings
                                    .remember_file(doc.path.as_deref().unwrap_or(&path));
                                let selected = self.documents.get(self.active).map(|doc| doc.id);
                                let focus = self.open_focus_id == Some(doc.id);
                                let index =
                                    self.documents.partition_point(|other| other.id < doc.id);
                                self.documents.insert(index, doc);
                                self.active = if focus {
                                    index
                                } else {
                                    selected
                                        .and_then(|id| {
                                            self.documents.iter().position(|doc| doc.id == id)
                                        })
                                        .unwrap_or(0)
                                };
                                self.focus_editor |= focus;
                            } else if self.open_focus_id == Some(doc.id)
                                && let Some(index) = self
                                    .documents
                                    .iter()
                                    .position(|other| other.path == doc.path)
                            {
                                self.active = index;
                                self.focus_editor = true;
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Loaded::Folder(path, result) => {
                    self.loading.remove(&path);
                    match result {
                        Ok(entries) => {
                            self.folders.insert(path, entries);
                        }
                        Err(error) => {
                            // An empty cached result prevents retry/thread loops. Refresh retries.
                            self.folders.insert(path, Vec::new());
                            self.error = Some(error);
                        }
                    }
                }
                Loaded::Root(id, requested, path, result) => {
                    self.loading.remove(&requested);
                    if id < self.root_request {
                        continue;
                    }
                    self.root_request = id;
                    self.root = Some(path.clone());
                    self.folders.clear();
                    self.expanded.clear();
                    self.settings.show_sidebar = true;
                    self.project_open = self.settings.zen;
                    self.settings.remember_folder(&path);
                    match result {
                        Ok(entries) => {
                            self.folders.insert(path, entries);
                        }
                        Err(error) => {
                            self.folders.insert(path, Vec::new());
                            self.error = Some(error);
                        }
                    }
                }
            }
        }
    }

    fn new_document(&mut self) {
        self.open_focus_id = None;
        self.documents.push(Document::blank(self.next_id));
        self.next_id += 1;
        self.active = self.documents.len() - 1;
        self.focus_editor = true;
        if self.view == View::Preview {
            self.view = View::Split;
        }
    }

    fn pick_files(&mut self, ctx: &egui::Context) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown", "mdown", "mkd", "txt"]);
        if let Some(paths) = dialog.pick_files() {
            for path in paths {
                self.open_path(path, ctx);
            }
        }
    }

    fn pick_folder(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.open_path(path, ctx);
        }
    }

    fn save(&mut self, index: usize, save_as: bool) -> bool {
        if index >= self.documents.len() {
            return false;
        }
        let doc = &self.documents[index];
        if self.autosave.is_saving(doc.id) {
            // Serialize writes for this document, including Save As.
            if let Some((_, queued_save_as)) =
                self.deferred_saves.iter_mut().find(|(id, _)| *id == doc.id)
            {
                *queued_save_as |= save_as;
            } else {
                self.deferred_saves.push((doc.id, save_as));
            }
            self.notice = format!("Сохраняем: {}", doc.title());
            return false;
        }
        let target = if save_as || doc.path.is_none() {
            let mut dialog = rfd::FileDialog::new()
                .add_filter("Markdown", &["md"])
                .set_file_name(
                    doc.path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .unwrap_or(std::ffi::OsStr::new("untitled.md"))
                        .to_string_lossy(),
                );
            if let Some(parent) = doc
                .path
                .as_ref()
                .and_then(|p| p.parent())
                .or(self.root.as_deref())
            {
                dialog = dialog.set_directory(parent);
            }
            let Some(path) = dialog.save_file() else {
                return false;
            };
            path
        } else {
            doc.path.clone().unwrap()
        };
        let canonical = std::fs::canonicalize(&target).unwrap_or(target.clone());
        if self
            .documents
            .iter()
            .enumerate()
            .any(|(other, doc)| other != index && doc.path.as_ref() == Some(&canonical))
        {
            self.error = Some("Этот файл уже открыт в другой вкладке. Выберите другое имя.".into());
            return false;
        }
        let same_path = self.documents[index].path.as_ref() == Some(&canonical);
        let overwrite = !same_path && target.exists();
        if overwrite
            && rfd::MessageDialog::new()
                .set_title("Заменить файл?")
                .set_description(format!(
                    "{} уже существует. Заменить его содержимое?",
                    target.display()
                ))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                != rfd::MessageDialogResult::Yes
        {
            return false;
        }
        match self.documents[index].save(&target, overwrite) {
            Ok(()) => {
                self.autosave.saved_manually(self.documents[index].id);
                self.settings.remember_file(&canonical);
                self.notice = format!("Сохранено: {}", target.display());
                // Refresh the containing directory if it is visible in the project tree.
                if let Some(parent) = canonical.parent() {
                    self.folders.remove(parent);
                }
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn receive_autosaves(&mut self) {
        for outcome in self.autosave.poll() {
            let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == outcome.id) else {
                continue;
            };
            match outcome.result {
                Ok(saved) => {
                    if doc.accept_saved_snapshot(saved) {
                        self.notice = format!("Автосохранено: {}", outcome.title);
                    }
                }
                Err(error) => {
                    let error = format!(
                        "Автосохранение «{}» не удалось. Правки остаются в редакторе.\n\n{}",
                        outcome.title, error
                    );
                    if let Some(existing) = &mut self.error {
                        existing.push_str(&format!("\n\n{error}"));
                    } else {
                        self.error = Some(error);
                    }
                }
            }
        }
        let queued = std::mem::take(&mut self.deferred_saves);
        for (id, save_as) in queued {
            if self.autosave.is_saving(id) {
                self.deferred_saves.push((id, save_as));
            } else if let Some(index) = self.documents.iter().position(|doc| doc.id == id) {
                self.save(index, save_as);
            }
        }
    }

    fn request_close(&mut self, id: u64) {
        if let Some(index) = self.documents.iter().position(|doc| doc.id == id) {
            if self.documents[index].is_dirty() || self.autosave.is_saving(id) {
                self.active = index;
                self.pending = Some(Pending::Close(id));
            } else {
                self.remove_document(index);
            }
        }
    }

    fn remove_document(&mut self, index: usize) {
        let id = self.documents[index].id;
        self.autosave.forget(id);
        self.deferred_saves.retain(|(pending, _)| *pending != id);
        self.syntax.remove(self.documents[index].id);
        self.syntax
            .remove(crate::live_preview::syntax_id(self.documents[index].id));
        self.live_preview.remove(self.documents[index].id);
        self.documents.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.documents.len().saturating_sub(1));
    }

    fn next_dirty(&self) -> Option<usize> {
        match self.pending {
            Some(Pending::Close(id)) => self.documents.iter().position(|doc| {
                doc.id == id && (doc.is_dirty() || self.autosave.is_saving(doc.id))
            }),
            Some(Pending::Quit) => self
                .documents
                .iter()
                .position(|doc| doc.is_dirty() || self.autosave.is_saving(doc.id)),
            None => None,
        }
    }

    fn finish_pending(&mut self, ctx: &egui::Context) {
        if self.next_dirty().is_some() {
            return;
        }
        match self.pending.take() {
            Some(Pending::Close(id)) => {
                if let Some(index) = self.documents.iter().position(|doc| doc.id == id) {
                    self.remove_document(index);
                }
            }
            Some(Pending::Quit) => {
                self.allow_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |key, shift| {
            ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers {
                        shift,
                        ..Modifiers::COMMAND
                    },
                    key,
                ))
            })
        };
        if pressed(Key::P, true) {
            self.commands_open = !self.commands_open;
        }
        if pressed(Key::H, true) {
            self.settings.zen = !self.settings.zen;
        }
        let tab = |shift| {
            ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers {
                        shift,
                        ..Modifiers::CTRL
                    },
                    Key::Tab,
                ))
            })
        };
        let previous = tab(true);
        if (previous || tab(false)) && !self.documents.is_empty() {
            self.active = if previous {
                (self.active + self.documents.len() - 1) % self.documents.len()
            } else {
                (self.active + 1) % self.documents.len()
            };
            self.open_focus_id = None;
            self.focus_editor = true;
        }
        if pressed(Key::O, true) {
            self.pick_folder(ctx);
        } else if pressed(Key::O, false) {
            self.pick_files(ctx);
        }
        if pressed(Key::S, true) {
            self.save(self.active, true);
        } else if pressed(Key::S, false) {
            self.save(self.active, false);
        }
        if pressed(Key::N, false) {
            self.new_document();
        }
        if pressed(Key::W, false)
            && let Some(doc) = self.documents.get(self.active)
        {
            self.request_close(doc.id);
        }
        if pressed(Key::Q, false) {
            self.pending = Some(Pending::Quit);
        }
        if pressed(Key::B, false) {
            if self.settings.zen {
                self.project_open = !self.project_open;
            } else {
                self.settings.show_sidebar = !self.settings.show_sidebar;
            }
        }
        if pressed(Key::F, false) {
            self.find_open = true;
            self.commands_open = false;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("find")));
        }
        if pressed(Key::Num1, false) {
            self.view = View::Edit;
        }
        if pressed(Key::Num2, false) {
            self.view = View::Split;
        }
        if pressed(Key::Num3, false) {
            self.view = View::Preview;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.find_open = false;
            self.commands_open = false;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("mde")
                    .strong()
                    .size(21.0)
                    .color(crate::themes::accent(&self.settings.theme)),
            );
            ui.separator();
            if ui.button("Новый").on_hover_text("⌘/Ctrl N").clicked() {
                self.new_document();
            }
            if ui.button("Файлы…").on_hover_text("⌘/Ctrl O").clicked() {
                self.pick_files(&ctx);
            }
            if ui
                .button("Папка…")
                .on_hover_text("⌘/Ctrl Shift O")
                .clicked()
            {
                self.pick_folder(&ctx);
            }
            ui.menu_button("Недавние", |ui| {
                if self.settings.recent_files.is_empty() && self.settings.recent_folders.is_empty()
                {
                    ui.weak("Пока ничего нет");
                }
                for path in self.settings.recent_files.clone() {
                    if ui.button(path.display().to_string()).clicked() {
                        self.open_path(path, &ctx);
                        ui.close();
                    }
                }
                if !self.settings.recent_folders.is_empty() {
                    ui.separator();
                    ui.weak("Папки");
                }
                for path in self.settings.recent_folders.clone() {
                    if ui.button(path.display().to_string()).clicked() {
                        self.open_path(path, &ctx);
                        ui.close();
                    }
                }
            });
            ui.add_enabled_ui(!self.documents.is_empty(), |ui| {
                if ui.button("Сохранить").on_hover_text("⌘/Ctrl S").clicked() {
                    self.save(self.active, false);
                }
                ui.menu_button("···", |ui| {
                    if ui.button("Сохранить как…").clicked() {
                        self.save(self.active, true);
                        ui.close();
                    }
                    if ui.button("Найти / заменить").clicked() {
                        self.find_open = true;
                        ui.close();
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.theme_picker(ui);
                if ui
                    .button("Скрыть")
                    .on_hover_text("⌘/Ctrl Shift H")
                    .clicked()
                {
                    self.settings.zen = true;
                }
                if ui
                    .selectable_label(self.settings.show_sidebar, "Панель")
                    .clicked()
                {
                    self.settings.show_sidebar = !self.settings.show_sidebar;
                }
            });
        });
    }

    fn theme_picker(&mut self, ui: &mut egui::Ui) {
        let previous = self.settings.theme.clone();
        egui::ComboBox::from_id_salt(ui.id().with("theme"))
            .selected_text(crate::themes::label(&self.settings.theme))
            .show_ui(ui, |ui| {
                for &(id, label) in crate::themes::THEMES {
                    ui.selectable_value(&mut self.settings.theme, id.to_owned(), label);
                }
            });
        if self.settings.theme != previous {
            self.settings.dark = crate::themes::apply(ui.ctx(), &self.settings.theme);
        }
    }

    fn command_palette(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("commands")).show(ctx, |ui| {
            ui.set_width(570.0);
            ui.horizontal(|ui| {
                ui.heading("Управление");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        close = true;
                    }
                    ui.weak("Esc");
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Файлы…").on_hover_text("⌘/Ctrl O").clicked() {
                    self.pick_files(ctx);
                    close = true;
                }
                if ui
                    .button("Папка…")
                    .on_hover_text("⌘/Ctrl Shift O")
                    .clicked()
                {
                    self.pick_folder(ctx);
                    close = true;
                }
                if ui.button("Новый").on_hover_text("⌘/Ctrl N").clicked() {
                    self.new_document();
                    close = true;
                }
                if ui
                    .add_enabled(!self.documents.is_empty(), egui::Button::new("Сохранить"))
                    .clicked()
                {
                    self.save(self.active, false);
                    close = true;
                }
                if ui
                    .add_enabled(
                        !self.documents.is_empty(),
                        egui::Button::new("Сохранить как…"),
                    )
                    .clicked()
                {
                    self.save(self.active, true);
                    close = true;
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Режим");
                for (view, label) in [
                    (View::Edit, "Редактор"),
                    (View::Split, "Вместе"),
                    (View::Preview, "Просмотр"),
                ] {
                    if ui.selectable_value(&mut self.view, view, label).clicked() {
                        close = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Тема");
                self.theme_picker(ui);
            });
            ui.horizontal(|ui| {
                ui.label("Шрифт");
                ui.add(egui::Slider::new(&mut self.settings.font_size, 12.0..=24.0).suffix(" px"));
            });
            ui.checkbox(&mut self.settings.zen, "Скрыть интерфейс · ⌘/Ctrl Shift H");
            if !self.settings.zen {
                ui.checkbox(&mut self.settings.show_sidebar, "Панель проекта");
            } else if self.root.is_some() && ui.button("Файлы проекта · ⌘/Ctrl B").clicked()
            {
                self.project_open = true;
                close = true;
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    if !self.documents.is_empty() {
                        ui.weak("Открытые файлы · Ctrl Tab / Ctrl Shift Tab");
                        let mut remove = None;
                        for (index, doc) in self.documents.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.active == index,
                                        format!(
                                            "{}{}",
                                            doc.title(),
                                            if doc.is_dirty() { " •" } else { "" }
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.active = index;
                                    self.open_focus_id = None;
                                    self.focus_editor = true;
                                    close = true;
                                }
                                if ui.small_button("×").clicked() {
                                    remove = Some(doc.id);
                                    close = true;
                                }
                            });
                        }
                        if let Some(id) = remove {
                            self.request_close(id);
                        }
                        ui.add_space(8.0);
                    }
                    ui.weak("Недавние файлы");
                    for path in self.settings.recent_files.clone().iter().take(10) {
                        if ui
                            .selectable_label(false, path.display().to_string())
                            .clicked()
                        {
                            self.open_path(path.clone(), ctx);
                            close = true;
                        }
                    }
                    if !self.settings.recent_folders.is_empty() {
                        ui.add_space(8.0);
                        ui.weak("Недавние папки");
                    }
                    for path in self.settings.recent_folders.clone().iter().take(5) {
                        if ui
                            .selectable_label(false, path.display().to_string())
                            .clicked()
                        {
                            self.open_path(path.clone(), ctx);
                            close = true;
                        }
                    }
                });
        });
        if close || response.should_close() {
            self.commands_open = false;
        }
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, doc) in self.documents.iter().enumerate() {
                        let title =
                            format!("{}{}", doc.title(), if doc.is_dirty() { "  •" } else { "" });
                        let tab = ui.selectable_label(self.active == index, title);
                        if let Some(path) = &doc.path {
                            tab.clone().on_hover_text(path.display().to_string());
                        }
                        if tab.clicked() {
                            self.active = index;
                        }
                        if tab.clicked_by(egui::PointerButton::Middle) {
                            close = Some(doc.id);
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("Закрыть вкладку")
                            .clicked()
                        {
                            close = Some(doc.id);
                        }
                        ui.separator();
                    }
                });
            });
        if let Some(id) = close {
            self.request_close(id);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.strong("ПРОЕКТ");
            if self.settings.zen && ui.small_button("×").clicked() {
                self.project_open = false;
            }
            if ui
                .small_button("↻")
                .on_hover_text("Обновить дерево")
                .clicked()
            {
                self.folders.clear();
            }
        });
        if let Some(root) = self.root.clone() {
            ui.label(
                RichText::new(
                    root.file_name()
                        .unwrap_or(root.as_os_str())
                        .to_string_lossy(),
                )
                .strong(),
            )
            .on_hover_text(root.display().to_string());
            ui.separator();
            let mut rows = Vec::new();
            self.collect_rows(&root, 0, &mut rows, &ctx);
            if rows.is_empty() {
                ui.weak(if self.loading.contains(&root) {
                    "Загрузка…"
                } else {
                    "Нет Markdown-файлов"
                });
            }
            let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
            egui::ScrollArea::both()
                .id_salt("project")
                .auto_shrink([false, false])
                .show_rows(ui, row_height, rows.len(), |ui, range| {
                    for (entry, depth) in &rows[range] {
                        let expanded = self.expanded.contains(&entry.path);
                        let active = self
                            .documents
                            .get(self.active)
                            .and_then(|doc| doc.path.as_ref())
                            == Some(&entry.path);
                        let prefix = if entry.is_dir {
                            if expanded { "v " } else { "> " }
                        } else {
                            "  "
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(*depth as f32 * 14.0);
                            let label = ui
                                .selectable_label(active, format!("{prefix}{}", entry.name))
                                .on_hover_text(entry.path.display().to_string());
                            if label.clicked() {
                                if entry.is_dir {
                                    if expanded {
                                        self.expanded.remove(&entry.path);
                                    } else {
                                        self.expanded.insert(entry.path.clone());
                                    }
                                } else {
                                    self.open_path(entry.path.clone(), &ctx);
                                }
                            }
                        });
                    }
                });
        } else {
            ui.add_space(10.0);
            ui.weak("Папка не открыта");
            if ui.button("Открыть папку…").clicked() {
                self.pick_folder(&ctx);
            }
            ui.add_space(24.0);
            ui.strong("НЕДАВНИЕ ФАЙЛЫ");
            ui.add_space(8.0);
            let mut open = None;
            for path in self.settings.recent_files.iter().take(12) {
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                if ui
                    .selectable_label(false, name)
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    open = Some(path.clone());
                }
            }
            if let Some(path) = open {
                self.open_path(path, &ctx);
            }
        }
    }

    fn collect_rows(
        &mut self,
        path: &Path,
        depth: usize,
        rows: &mut Vec<(FolderEntry, usize)>,
        ctx: &egui::Context,
    ) {
        if depth > 48 {
            return;
        }
        self.load_folder(path.to_path_buf(), ctx);
        let Some(entries) = self.folders.get(path).cloned() else {
            return;
        };
        for entry in entries {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            let path = entry.path.clone();
            rows.push((entry, depth));
            if expanded {
                self.collect_rows(&path, depth + 1, rows, ctx);
            }
        }
    }

    fn find_bar(&mut self, ui: &mut egui::Ui) {
        let mut next = false;
        let mut replace = false;
        ui.horizontal(|ui| {
            ui.label("Найти");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(egui::Id::new("find"))
                    .desired_width(180.0),
            );
            if response.changed() {
                self.find_offset = 0;
            }
            let enter = (response.has_focus() || response.lost_focus())
                && ui.input(|i| i.key_pressed(Key::Enter));
            if enter {
                ui.input_mut(|i| {
                    i.consume_key(Modifiers::NONE, Key::Enter);
                });
            }
            next = ui.button("Далее").clicked() || enter;
            ui.label("Заменить");
            ui.add(egui::TextEdit::singleline(&mut self.replacement).desired_width(180.0));
            replace = ui.button("Все").clicked();
            if let Some(doc) = self.documents.get(self.active) {
                let count = if self.query.is_empty() {
                    0
                } else {
                    doc.text.matches(&self.query).count()
                };
                ui.weak(format!("{count} совпадений"));
            }
            if ui.small_button("×").clicked() {
                self.find_open = false;
            }
        });
        if self.query.is_empty() {
            return;
        }
        if let Some(doc) = self.documents.get_mut(self.active) {
            if replace {
                doc.text = doc.text.replace(&self.query, &self.replacement);
                self.find_offset = 0;
            }
            if next {
                let offset = self.find_offset.min(doc.text.len());
                let found = doc
                    .text
                    .get(offset..)
                    .and_then(|text| text.find(&self.query))
                    .map(|i| offset + i)
                    .or_else(|| doc.text.find(&self.query));
                if let Some(byte) = found {
                    let start = doc.text[..byte].chars().count();
                    let end = start + self.query.chars().count();
                    self.find_selection = Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(start),
                        egui::text::CCursor::new(end),
                    ));
                    self.find_offset = byte + self.query.len();
                    self.focus_editor = true;
                    if self.view == View::Preview {
                        self.view = View::Split;
                    }
                }
            }
        }
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        if self.documents.is_empty() {
            self.welcome(ui);
            return;
        }
        if !self.settings.zen {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view, View::Edit, "Редактор")
                    .on_hover_text("⌘/Ctrl 1");
                ui.selectable_value(&mut self.view, View::Split, "Вместе")
                    .on_hover_text("⌘/Ctrl 2");
                ui.selectable_value(&mut self.view, View::Preview, "Просмотр")
                    .on_hover_text("⌘/Ctrl 3");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.settings.font_size, 12.0..=24.0)
                            .show_value(true)
                            .suffix(" px"),
                    );
                });
            });
            ui.separator();
        }
        if self.find_open {
            self.find_bar(ui);
            ui.separator();
        }
        let doc = &mut self.documents[self.active];
        let font_size = self.settings.font_size;
        let focus = std::mem::take(&mut self.focus_editor);
        let selection = self.find_selection.take();
        match self.view {
            View::Edit => editor(
                ui,
                doc,
                &mut self.syntax,
                &self.settings.theme,
                font_size,
                focus,
                selection,
            ),
            View::Preview => {
                if preview(
                    ui,
                    doc,
                    &mut self.cache,
                    &mut self.live_preview,
                    &mut self.syntax,
                    (&self.settings.theme, font_size),
                    self.large_preview_allowed.contains(&doc.id),
                ) {
                    self.large_preview_allowed.insert(doc.id);
                }
            }
            View::Split => {
                egui::Panel::left("source")
                    .resizable(true)
                    .default_size(ui.available_width() * 0.5)
                    .min_size(180.0)
                    .show(ui, |ui| {
                        editor(
                            ui,
                            doc,
                            &mut self.syntax,
                            &self.settings.theme,
                            font_size,
                            focus,
                            selection,
                        )
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    if preview(
                        ui,
                        doc,
                        &mut self.cache,
                        &mut self.live_preview,
                        &mut self.syntax,
                        (&self.settings.theme, font_size),
                        self.large_preview_allowed.contains(&doc.id),
                    ) {
                        self.large_preview_allowed.insert(doc.id);
                    }
                });
            }
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.add_space((ui.available_height() * 0.18).max(24.0));
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("mde")
                    .size(64.0)
                    .strong()
                    .color(crate::themes::accent(&self.settings.theme)),
            );
            ui.add_space(12.0);
            ui.heading("Markdown. Без лишнего.");
            ui.weak("Пишите текст и сразу смотрите результат.");
            ui.add_space(28.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 345.0).max(0.0) / 2.0);
                if ui.button("Открыть файлы…").clicked() {
                    self.pick_files(&ctx);
                }
                if ui.button("Открыть папку…").clicked() {
                    self.pick_folder(&ctx);
                }
                if ui.button("Новый файл").clicked() {
                    self.new_document();
                }
            });
            ui.add_space(18.0);
            ui.weak("Можно перетащить файлы или папку в окно");
            ui.add_space(8.0);
            ui.weak("⌘/Ctrl Shift P или правая кнопка мыши — управление и темы");
            ui.add_space(28.0);
            for path in self.settings.recent_files.clone().iter().take(5) {
                if ui.link(path.display().to_string()).clicked() {
                    self.open_path(path.clone(), &ctx);
                }
            }
            if !self.loading.is_empty() {
                ui.spinner();
                ui.weak("Открываем…");
            }
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            if self.error.is_some() {
                self.error = None;
            } else {
                self.pending = None;
            }
            return;
        }
        if let Some(error) = self.error.clone() {
            egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                ui.set_max_width(480.0);
                ui.heading("Не удалось выполнить действие");
                ui.add_space(8.0);
                ui.label(error);
                ui.add_space(12.0);
                if ui.button("Понятно").clicked() {
                    self.error = None;
                }
            });
            return;
        }
        if let Some(index) = self.next_dirty() {
            let title = self.documents[index].title();
            if self.autosave.is_saving(self.documents[index].id) {
                egui::Modal::new(egui::Id::new("saving_before_close")).show(ctx, |ui| {
                    ui.heading("Сохраняем…");
                    ui.label(format!("Дождитесь завершения записи «{title}»."));
                    if ui.button("Отмена").clicked() {
                        self.pending = None;
                    }
                });
                return;
            }
            egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                ui.heading("Сохранить изменения?");
                ui.label(format!("В «{title}» есть несохранённые изменения."));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() {
                        self.save(index, false);
                    }
                    if ui.button("Не сохранять").clicked() {
                        // A discarded document is removed, so Cancel on the next quit prompt
                        // cannot leave it falsely marked as saved.
                        self.remove_document(index);
                    }
                    if ui.button("Отмена").clicked() {
                        self.pending = None;
                    }
                });
            });
        }
        self.finish_pending(ctx);
    }
}

impl eframe::App for MdeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.first_frame {
            self.first_frame = false;
            if std::env::var_os("MDE_STARTUP_LOG").is_some() {
                eprintln!(
                    "mde: first UI frame in {:.1} ms",
                    self.started.elapsed().as_secs_f64() * 1000.0
                );
            }
            // Wait until after the first window frame to open supplied files.
            ctx.request_repaint();
        } else {
            for path in std::mem::take(&mut self.initial_paths) {
                self.open_path(path, &ctx);
            }
        }
        if !self.documents.is_empty()
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    || i.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Text(_) | egui::Event::Key { pressed: true, .. }
                        )
                    })
            })
        {
            self.open_focus_id = None;
        }
        self.receive();
        self.receive_autosaves();
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        if self.pending.is_none() && self.error.is_none() {
            self.shortcuts(&ctx);
            if ctx.input(|input| input.pointer.secondary_clicked()) {
                self.commands_open = true;
            }
        }
        for path in ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect::<Vec<_>>()
        }) {
            self.open_path(path, &ctx);
        }
        if !self.settings.zen {
            egui::Panel::top("toolbar").show(ui, |ui| {
                self.toolbar(ui);
            });
            if !self.documents.is_empty() {
                egui::Panel::top("tabs").show(ui, |ui| self.tabs(ui));
            }
            egui::Panel::bottom("status").show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(doc) = self.documents.get(self.active) {
                        ui.weak(
                            doc.path.as_ref().map_or_else(
                                || "Новый документ".into(),
                                |p| p.display().to_string(),
                            ),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(if doc.is_dirty() {
                                "Изменён"
                            } else {
                                "Сохранён"
                            });
                            ui.separator();
                            ui.weak(format!("{} символов", doc.text.chars().count()));
                            ui.weak("UTF-8");
                        });
                    } else {
                        ui.weak("⌘/Ctrl O — файл     ⌘/Ctrl Shift O — папка     ⌘/Ctrl N — новый");
                    }
                });
                if !self.notice.is_empty() {
                    ui.weak(&self.notice);
                }
            });
            if self.settings.show_sidebar {
                egui::Panel::left("sidebar")
                    .default_size(220.0)
                    .min_size(160.0)
                    .max_size(420.0)
                    .show(ui, |ui| self.sidebar(ui));
            }
        }
        if self.settings.zen && self.project_open {
            egui::Panel::left("sidebar")
                .default_size(220.0)
                .min_size(160.0)
                .max_size(420.0)
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.workspace(ui));
        let title = self.documents.get(self.active).map_or_else(
            || "mde".into(),
            |doc| {
                format!(
                    "{}{} — mde",
                    doc.title(),
                    if doc.is_dirty() { " •" } else { "" }
                )
            },
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        self.dialogs(&ctx);
        if self.pending.is_none() && self.error.is_none() && self.commands_open {
            self.command_palette(&ctx);
        }
        self.autosave.update(
            &self.documents,
            Instant::now(),
            &ctx,
            self.pending.is_none() && self.error.is_none(),
        );
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(error) = self.settings.save() {
            eprintln!("mde: {error}");
        }
    }
}

fn editor(
    ui: &mut egui::Ui,
    doc: &mut Document,
    syntax: &mut crate::syntax::Syntax,
    theme: &str,
    font_size: f32,
    focus: bool,
    selection: Option<egui::text::CCursorRange>,
) {
    egui::ScrollArea::both()
        .id_salt(("editor_scroll", doc.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let id = egui::Id::new(("editor", doc.id));
            if focus {
                ui.memory_mut(|memory| memory.request_focus(id));
            }
            let doc_id = doc.id;
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                syntax.layout(ui, doc_id, text.as_str(), theme, font_size, wrap_width)
            };
            let mut output = egui::TextEdit::multiline(&mut doc.text)
                .id(id)
                .font(FontId::monospace(font_size))
                .layouter(&mut layouter)
                .desired_width(ui.available_width())
                .desired_rows(28)
                .cursor_at_end(false)
                .lock_focus(true)
                .frame(egui::Frame::NONE)
                .hint_text("# Начните писать…")
                .show(ui);
            if focus {
                output.response.request_focus();
            }
            if let Some(range) = selection {
                output.state.cursor.set_char_range(Some(range));
                output.state.clone().store(ui.ctx(), id);
                output.cursor_range = Some(range);
                ui.ctx().request_repaint();
            }
            if output.response.has_focus()
                && let Some(range) = output.cursor_range
            {
                let cursor = range.primary;
                if ui.input(|i| {
                    i.key_pressed(Key::Enter)
                        || i.key_pressed(Key::ArrowDown)
                        || i.key_pressed(Key::ArrowUp)
                }) || focus
                {
                    let rect = output
                        .galley
                        .pos_from_cursor(cursor)
                        .translate(output.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect, None);
                }
            }
        });
}

fn preview(
    ui: &mut egui::Ui,
    doc: &mut Document,
    cache: &mut CommonMarkCache,
    live: &mut crate::live_preview::LivePreview,
    syntax: &mut crate::syntax::Syntax,
    appearance: (&str, f32),
    allow_large: bool,
) -> bool {
    let (theme, font_size) = appearance;
    if doc.text.len() > 512 * 1024 && !allow_large {
        ui.add_space(24.0);
        ui.heading("Большой документ");
        ui.label("Предпросмотр выключен, чтобы файл открылся быстрее. Полный рендеринг может занять время.");
        return ui.button("Включить предпросмотр").clicked();
    }
    ui.add_space(8.0);
    egui::ScrollArea::both()
        .id_salt(("preview", doc.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(font_size));
            ui.style_mut().text_styles.insert(
                egui::TextStyle::Heading,
                FontId::proportional(font_size * 1.8),
            );
            ui.style_mut().text_styles.insert(
                egui::TextStyle::Monospace,
                FontId::monospace(font_size - 1.0),
            );
            ui.add_space(6.0);
            live.show(ui, doc, cache, syntax, theme, font_size);
        });
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> MdeApp {
        MdeApp::with_settings(
            &egui::Context::default(),
            vec![],
            Instant::now(),
            Settings::default(),
        )
    }

    fn loaded_doc(id: u64) -> Document {
        let mut doc = Document::blank(id);
        doc.path = Some(PathBuf::from(format!("/nonexistent/mde-test-{id}.md")));
        doc
    }

    #[test]
    fn background_completion_preserves_request_order_and_selection() {
        let mut app = app();
        app.open_focus_id = Some(3);
        for id in [3, 1, 2] {
            let doc = loaded_doc(id);
            app.tx
                .send(Loaded::File(doc.path.clone().unwrap(), Ok(doc)))
                .unwrap();
            app.receive();
        }
        assert_eq!(
            app.documents.iter().map(|doc| doc.id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(app.documents[app.active].id, 3);
    }

    #[test]
    fn late_open_does_not_steal_user_selection() {
        let mut app = app();
        app.documents.push(loaded_doc(2));
        app.open_focus_id = None;
        let doc = loaded_doc(1);
        app.tx
            .send(Loaded::File(doc.path.clone().unwrap(), Ok(doc)))
            .unwrap();
        app.receive();
        assert_eq!(app.documents[app.active].id, 2);
    }

    #[test]
    fn failed_folder_is_cached_until_explicit_refresh() {
        let mut app = app();
        let path = PathBuf::from("/unreadable");
        app.loading.insert(path.clone());
        app.tx
            .send(Loaded::Folder(
                path.clone(),
                Err("permission denied".into()),
            ))
            .unwrap();
        app.receive();
        app.load_folder(path.clone(), &egui::Context::default());
        assert!(!app.loading.contains(&path));
        assert!(app.folders.contains_key(&path));
        assert_eq!(app.error.as_deref(), Some("permission denied"));
    }

    #[test]
    fn close_dirty_document_waits_and_cancel_keeps_text() {
        let mut app = app();
        let mut doc = loaded_doc(1);
        doc.text = "несохранённый текст".into();
        app.documents.push(doc);
        app.request_close(1);
        app.finish_pending(&egui::Context::default());
        assert_eq!(app.next_dirty(), Some(0));
        assert_eq!(app.documents.len(), 1);
        app.pending = None;
        assert!(app.documents[0].is_dirty());
        assert_eq!(app.documents[0].text, "несохранённый текст");
    }

    #[test]
    fn manual_save_and_close_wait_for_background_write_without_losing_new_edits() {
        let path = std::env::temp_dir().join(format!(
            "mde-app-autosave-{}-{}.md",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        std::fs::write(&path, "Original").unwrap();
        let mut app = app();
        app.documents.push(Document::open(1, &path).unwrap());
        app.documents[0].text = "Background snapshot".into();
        let ctx = egui::Context::default();
        let now = Instant::now();
        app.autosave.update(&app.documents, now, &ctx, true);
        app.autosave
            .update(&app.documents, now + crate::autosave::DELAY, &ctx, true);
        app.documents[0].text = "Latest editor text".into();
        assert!(!app.save(0, false));
        assert_eq!(app.deferred_saves, vec![(1, false)]);
        app.request_close(1);
        app.finish_pending(&ctx);
        assert_eq!(app.documents.len(), 1);
        assert!(!app.allow_quit);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while app.autosave.is_saving(1) {
            app.receive_autosaves();
            assert!(Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(app.deferred_saves.is_empty());
        assert!(!app.documents[0].is_dirty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "Latest editor text"
        );
        app.finish_pending(&ctx);
        assert!(app.documents.is_empty());
    }

    #[test]
    fn closing_earlier_tab_keeps_active_document() {
        let mut app = app();
        app.documents = vec![loaded_doc(1), loaded_doc(2), loaded_doc(3)];
        app.active = 2;
        app.request_close(1);
        assert_eq!(app.documents[app.active].id, 3);
    }

    #[test]
    fn quit_checks_every_dirty_document() {
        let mut app = app();
        app.documents = vec![loaded_doc(1), loaded_doc(2)];
        for doc in &mut app.documents {
            doc.text = "changed".into();
        }
        app.pending = Some(Pending::Quit);
        app.remove_document(app.next_dirty().unwrap());
        app.finish_pending(&egui::Context::default());
        assert!(!app.allow_quit);
        assert_eq!(app.next_dirty(), Some(0));
        app.remove_document(0);
        app.finish_pending(&egui::Context::default());
        assert!(app.allow_quit);
    }

    #[test]
    fn markdown_preview_and_three_workspace_modes_render() {
        let mut app = app();
        let mut doc = loaded_doc(1);
        doc.text = include_str!("../examples/demo.md").into();
        app.documents.push(doc);
        for view in [View::Edit, View::Split, View::Preview] {
            app.view = view;
            egui::__run_test_ui(|ui| app.workspace(ui));
        }
    }

    #[test]
    fn enter_in_find_selects_unicode_match_without_editing_document() {
        let ctx = egui::Context::default();
        let mut app = MdeApp::with_settings(&ctx, vec![], Instant::now(), Settings::default());
        let mut doc = loaded_doc(1);
        doc.text = "Начало\n\n## Таблица\n\nКонец".into();
        let original = doc.text.clone();
        app.documents.push(doc);
        app.find_open = true;
        app.query = "Таблица".into();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.workspace(ui));
        output.textures_delta.clear();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("find")));
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.workspace(ui));
        output.textures_delta.clear();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.workspace(ui));
        output.textures_delta.clear();
        let state = egui::TextEdit::load_state(&ctx, egui::Id::new(("editor", 1_u64))).unwrap();
        let range = state.cursor.char_range().unwrap().as_sorted_char_range();
        let start: usize = range.start.into();
        let end: usize = range.end.into();
        assert_eq!(
            original
                .chars()
                .skip(start)
                .take(end - start)
                .collect::<String>(),
            "Таблица"
        );
        assert_eq!(app.documents[0].text, original);
    }

    #[test]
    fn zen_defaults_and_preferences_round_trip() {
        let settings = Settings::default();
        assert!(settings.zen);
        let changed = Settings {
            zen: false,
            theme: "nord".into(),
            ..settings
        };
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&changed).unwrap()).unwrap();
        assert!(!restored.zen);
        assert_eq!(restored.theme, "nord");
        // Preferences from the earlier version acquire safe new defaults.
        let old: Settings = serde_json::from_str(r#"{"dark":true}"#).unwrap();
        assert!(old.zen);
        assert_eq!(old.theme, "catppuccin-mocha");
    }

    #[test]
    fn hidden_workspace_paints_document_without_mode_controls() {
        fn texts(shape: &egui::epaint::Shape, output: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(text) => output.push(text.galley.job.text.clone()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, output);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut app = MdeApp::with_settings(&ctx, vec![], Instant::now(), Settings::default());
        let mut doc = loaded_doc(1);
        doc.text = "# Document".into();
        app.documents.push(doc);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.workspace(ui));
        let mut labels = Vec::new();
        for shape in &output.shapes {
            texts(&shape.shape, &mut labels);
        }
        output.textures_delta.clear();
        assert!(labels.iter().any(|label| label.contains("Document")));
        assert!(!labels.iter().any(|label| {
            ["Редактор", "Вместе", "Просмотр", "MARKDOWN", "ПРОСМОТР"].contains(&label.as_str())
        }));
    }
}
