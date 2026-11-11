//! Debounced, lazy background saving. No filesystem work or timer while idle.
use crate::model::Document;
use eframe::egui;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

pub const DELAY: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct AutoSave {
    tracked: HashMap<u64, Tracked>,
    saving: HashSet<u64>,
    worker: Option<Worker>,
    failures: Vec<Outcome>,
}

struct Tracked {
    text: String,
    path: PathBuf,
    due: Option<Instant>,
    blocked: bool,
}

struct Worker {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Outcome>,
}

struct Job {
    document: Document,
    context: egui::Context,
}

pub struct Outcome {
    pub id: u64,
    pub title: String,
    pub result: Result<Document, String>,
}

impl Worker {
    fn new() -> Result<Self, String> {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (results, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("mde-autosave".to_owned())
            .spawn(move || {
                while let Ok(mut job) = jobs.recv() {
                    let id = job.document.id;
                    let title = job.document.title();
                    let path = job.document.path.clone().expect("named autosave document");
                    let result = job.document.save(&path, false).map(|()| job.document);
                    if results.send(Outcome { id, title, result }).is_err() {
                        break;
                    }
                    job.context.request_repaint();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self { tx, rx })
    }
}

impl AutoSave {
    pub fn is_saving(&self, id: u64) -> bool {
        self.saving.contains(&id)
    }

    pub fn forget(&mut self, id: u64) {
        self.tracked.remove(&id);
    }

    pub fn saved_manually(&mut self, id: u64) {
        self.tracked.remove(&id);
    }

    pub fn poll(&mut self) -> Vec<Outcome> {
        let mut outcomes = std::mem::take(&mut self.failures);
        if let Some(worker) = &self.worker {
            outcomes.extend(worker.rx.try_iter());
        }
        for outcome in &outcomes {
            self.saving.remove(&outcome.id);
            if outcome.result.is_err()
                && let Some(tracked) = self.tracked.get_mut(&outcome.id)
            {
                // Avoid repeated writes and error dialogs until a manual save or reload.
                tracked.blocked = true;
                tracked.due = None;
            }
        }
        outcomes
    }

    pub fn update(
        &mut self,
        documents: &[Document],
        now: Instant,
        context: &egui::Context,
        allow_save: bool,
    ) {
        self.tracked
            .retain(|id, _| documents.iter().any(|doc| doc.id == *id));
        for document in documents {
            let Some(path) = &document.path else {
                self.tracked.remove(&document.id);
                continue;
            };
            if !document.is_dirty() && !self.is_saving(document.id) {
                self.tracked.remove(&document.id);
                continue;
            }
            let tracked = self.tracked.entry(document.id).or_insert_with(|| Tracked {
                text: document.text.clone(),
                path: path.clone(),
                due: Some(now + DELAY),
                blocked: false,
            });
            if tracked.path != *path {
                tracked.path.clone_from(path);
                tracked.blocked = false;
                tracked.due = Some(now + DELAY);
            }
            if tracked.text != document.text {
                tracked.text.clone_from(&document.text);
                if !tracked.blocked {
                    tracked.due = Some(now + DELAY);
                }
            }
        }
        if !allow_save {
            return;
        }
        for document in documents {
            if self.is_saving(document.id) {
                continue;
            }
            let Some(tracked) = self.tracked.get_mut(&document.id) else {
                continue;
            };
            if tracked.blocked || !tracked.due.is_some_and(|due| due <= now) {
                continue;
            }
            tracked.due = None;
            let job = Job {
                document: document.clone(),
                context: context.clone(),
            };
            let result = (|| {
                if self.worker.is_none() {
                    self.worker = Some(Worker::new()?);
                }
                self.worker
                    .as_ref()
                    .unwrap()
                    .tx
                    .send(job)
                    .map_err(|error| error.to_string())
            })();
            match result {
                Ok(()) => {
                    self.saving.insert(document.id);
                }
                Err(error) => {
                    self.failures.push(Outcome {
                        id: document.id,
                        title: document.title(),
                        result: Err(error),
                    });
                    context.request_repaint();
                }
            }
        }
        if let Some(due) = self
            .tracked
            .iter()
            .filter(|(id, tracked)| !self.is_saving(**id) && !tracked.blocked)
            .filter_map(|(_, tracked)| tracked.due)
            .min()
        {
            context.request_repaint_after(due.saturating_duration_since(now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct File(PathBuf);
    impl File {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mde-autosave-{}-{}.md",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, "Original").unwrap();
            Self(path)
        }
        fn document(&self, id: u64) -> Document {
            Document::open(id, &self.0).unwrap()
        }
    }
    impl Drop for File {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn complete(saves: &mut AutoSave) -> Outcome {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(outcome) = saves.poll().pop() {
                return outcome;
            }
            assert!(Instant::now() < deadline, "autosave worker timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn saves_exactly_after_one_second_of_inactivity_and_restarts_on_edit() {
        let file = File::new();
        let mut documents = vec![file.document(1)];
        let mut saves = AutoSave::default();
        let context = egui::Context::default();
        let now = Instant::now();
        saves.update(&documents, now, &context, true);
        assert!(saves.worker.is_none());
        documents[0].text = "First edit".into();
        saves.update(&documents, now, &context, true);
        saves.update(&documents, now + Duration::from_millis(999), &context, true);
        assert!(saves.worker.is_none());
        documents[0].text = "Последняя правка".into();
        saves.update(&documents, now + Duration::from_millis(999), &context, true);
        saves.update(
            &documents,
            now + Duration::from_millis(1998),
            &context,
            true,
        );
        assert!(saves.worker.is_none());
        saves.update(
            &documents,
            now + Duration::from_millis(1999),
            &context,
            true,
        );
        assert!(saves.is_saving(1));
        assert!(documents[0].accept_saved_snapshot(complete(&mut saves).result.unwrap()));
        assert_eq!(fs::read_to_string(&file.0).unwrap(), "Последняя правка");
        assert!(!documents[0].is_dirty());
    }

    #[test]
    fn edits_and_undo_during_save_are_not_lost() {
        let file = File::new();
        let mut documents = vec![file.document(1)];
        let mut saves = AutoSave::default();
        let context = egui::Context::default();
        let now = Instant::now();
        documents[0].text = "Snapshot".into();
        saves.update(&documents, now, &context, true);
        saves.update(&documents, now + DELAY, &context, true);
        documents[0].text = "Original".into(); // undo while the worker is saving
        saves.update(
            &documents,
            now + DELAY + Duration::from_millis(20),
            &context,
            true,
        );
        assert!(documents[0].accept_saved_snapshot(complete(&mut saves).result.unwrap()));
        assert_eq!(documents[0].text, "Original");
        assert!(documents[0].is_dirty());
        saves.update(
            &documents,
            now + DELAY + Duration::from_millis(1019),
            &context,
            true,
        );
        assert!(!saves.is_saving(1));
        saves.update(
            &documents,
            now + DELAY + Duration::from_millis(1020),
            &context,
            true,
        );
        documents[0].accept_saved_snapshot(complete(&mut saves).result.unwrap());
        assert_eq!(fs::read_to_string(&file.0).unwrap(), "Original");
        assert!(!documents[0].is_dirty());
    }

    #[test]
    fn conflicts_do_not_overwrite_disk_or_repeat_automatically() {
        let file = File::new();
        let mut documents = vec![file.document(1)];
        let mut saves = AutoSave::default();
        let context = egui::Context::default();
        let now = Instant::now();
        documents[0].text = "My edit".into();
        saves.update(&documents, now, &context, true);
        fs::write(&file.0, "External edit").unwrap();
        saves.update(&documents, now + DELAY, &context, true);
        assert!(complete(&mut saves).result.is_err());
        assert_eq!(fs::read_to_string(&file.0).unwrap(), "External edit");
        assert!(documents[0].is_dirty());
        documents[0].text.push('!');
        saves.update(&documents, now + Duration::from_secs(20), &context, true);
        assert!(!saves.is_saving(1));
        assert!(saves.poll().is_empty());
    }

    #[test]
    fn pending_dialog_pauses_saving_and_resume_keeps_deadline() {
        let file = File::new();
        let mut documents = vec![file.document(1)];
        documents[0].text = "Changed".into();
        let mut saves = AutoSave::default();
        let context = egui::Context::default();
        let now = Instant::now();
        saves.update(&documents, now, &context, true);
        saves.update(&documents, now + DELAY, &context, false);
        assert!(saves.worker.is_none());
        saves.update(&documents, now + DELAY, &context, true);
        documents[0].accept_saved_snapshot(complete(&mut saves).result.unwrap());
        assert_eq!(fs::read_to_string(&file.0).unwrap(), "Changed");
    }

    #[test]
    fn save_as_reenables_autosave_after_a_conflict() {
        let original = File::new();
        let target = File::new();
        let mut documents = vec![original.document(1)];
        documents[0].text = "Changed".into();
        let mut saves = AutoSave::default();
        let context = egui::Context::default();
        let now = Instant::now();
        saves.update(&documents, now, &context, true);
        fs::write(&original.0, "External").unwrap();
        saves.update(&documents, now + DELAY, &context, true);
        assert!(complete(&mut saves).result.is_err());
        documents[0].save(&target.0, true).unwrap();
        saves.saved_manually(1);
        documents[0].text = "Next edit".into();
        saves.update(&documents, now + DELAY, &context, true);
        saves.update(&documents, now + DELAY + DELAY, &context, true);
        documents[0].accept_saved_snapshot(complete(&mut saves).result.unwrap());
        assert_eq!(fs::read_to_string(&original.0).unwrap(), "External");
        assert_eq!(fs::read_to_string(&target.0).unwrap(), "Next edit");
    }

    #[test]
    fn every_document_has_its_own_idle_deadline() {
        let first = File::new();
        let second = File::new();
        let mut documents = vec![first.document(1), second.document(2)];
        let mut saves = AutoSave::default();
        let context = egui::Context::default();
        let now = Instant::now();
        documents[0].text = "First edit".into();
        saves.update(&documents, now, &context, true);
        documents[1].text = "Second edit".into();
        saves.update(&documents, now + Duration::from_millis(500), &context, true);
        saves.update(&documents, now + DELAY, &context, true);
        assert!(saves.is_saving(1));
        assert!(!saves.is_saving(2));
        documents[0].accept_saved_snapshot(complete(&mut saves).result.unwrap());
        saves.update(
            &documents,
            now + DELAY + Duration::from_millis(500),
            &context,
            true,
        );
        assert!(saves.is_saving(2));
        documents[1].accept_saved_snapshot(complete(&mut saves).result.unwrap());
        assert_eq!(fs::read_to_string(&first.0).unwrap(), "First edit");
        assert_eq!(fs::read_to_string(&second.0).unwrap(), "Second edit");
    }

    #[test]
    fn unnamed_clean_reverted_and_closed_documents_do_not_schedule_writes() {
        let file = File::new();
        let mut documents = vec![file.document(1), Document::blank(2)];
        let mut saves = AutoSave::default();
        let context = egui::Context::default();
        let now = Instant::now();
        documents[1].text = "Untitled".into();
        saves.update(&documents, now, &context, true);
        assert!(saves.tracked.is_empty());
        documents[0].text = "Changed".into();
        saves.update(&documents, now, &context, true);
        documents[0].text = "Original".into();
        saves.update(&documents, now + DELAY, &context, true);
        assert!(saves.tracked.is_empty());
        documents[0].text = "Changed again".into();
        saves.update(&documents, now + DELAY, &context, true);
        saves.update(&[], now + DELAY + DELAY, &context, true);
        assert!(saves.tracked.is_empty());
        assert!(saves.worker.is_none());
    }
}
