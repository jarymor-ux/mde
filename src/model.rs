//! File operations and persistent preferences, independent of the user interface.

use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const RECENT_LIMIT: usize = 20;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
struct DiskVersion {
    modified: Option<SystemTime>,
    len: u64,
    text: String,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub text: String,
    pub saved_text: String,
    disk_version: Option<DiskVersion>,
}

impl Document {
    pub fn blank(id: u64) -> Self {
        Self {
            id,
            path: None,
            text: String::new(),
            saved_text: String::new(),
            disk_version: None,
        }
    }

    pub fn open(id: u64, path: &Path) -> Result<Self, String> {
        let path =
            fs::canonicalize(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let version = read_version(&path)?;
        Ok(Self {
            id,
            path: Some(path),
            text: version.text.clone(),
            saved_text: version.text.clone(),
            disk_version: Some(version),
        })
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled.md".to_owned())
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// Accept a completed background save while preserving edits made since it began.
    pub fn accept_saved_snapshot(&mut self, snapshot: Self) -> bool {
        if self.id != snapshot.id
            || self.path != snapshot.path
            || snapshot.path.is_none()
            || snapshot.disk_version.is_none()
            || snapshot.is_dirty()
        {
            return false;
        }
        self.path = snapshot.path;
        self.saved_text = snapshot.saved_text;
        self.disk_version = snapshot.disk_version;
        true
    }

    pub fn save(&mut self, target: &Path, overwrite: bool) -> Result<(), String> {
        if self.text.len() as u64 > MAX_FILE_BYTES {
            return Err("The document exceeds the 32 MiB file limit.".to_owned());
        }
        // Resolve existing symlinks before replacement, leaving the link intact.
        let target = resolve_target(target)?;
        let same_document = self.path.as_ref() == Some(&target);
        let validate = || -> Result<(), String> {
            if overwrite {
                return Ok(());
            }
            if same_document {
                let expected = self.disk_version.as_ref().ok_or_else(|| {
                    "Cannot verify the original file; use Save As or confirm overwrite.".to_owned()
                })?;
                let current = read_version(&target).map_err(|_| {
                    "The file was changed or removed outside the editor; reload it or confirm overwrite."
                        .to_owned()
                })?;
                if current.len != expected.len
                    || current.modified != expected.modified
                    || current.text != expected.text
                {
                    return Err(
                        "The file was changed outside the editor; reload it or confirm overwrite."
                            .to_owned(),
                    );
                }
            } else if target.try_exists().map_err(|e| e.to_string())? {
                return Err("A file already exists at this location; confirm overwrite.".to_owned());
            }
            Ok(())
        };
        validate()?;
        let permissions = fs::metadata(&target)
            .ok()
            .map(|metadata| metadata.permissions());
        atomic_write(&target, self.text.as_bytes(), permissions, validate)?;
        // Writing already succeeded: record that fact even if a subsequent metadata
        // query fails, rather than reporting an unsaved document after replacement.
        let metadata = fs::metadata(&target).ok();
        self.disk_version = Some(DiskVersion {
            modified: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok()),
            len: self.text.len() as u64,
            text: self.text.clone(),
        });
        self.path = Some(target);
        self.saved_text.clone_from(&self.text);
        Ok(())
    }
}

fn read_version(path: &Path) -> Result<DiskVersion, String> {
    // Reject FIFOs and device nodes before opening them: a FIFO open can block.
    let metadata =
        fs::metadata(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a regular file.", path.display()));
    }
    let mut file = File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    if !before.is_file() {
        return Err(format!("{} is not a regular file.", path.display()));
    }
    if before.len() > MAX_FILE_BYTES {
        return Err("The file exceeds the 32 MiB file limit.".to_owned());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("The file exceeds the 32 MiB file limit.".to_owned());
    }
    let after = file.metadata().map_err(|e| e.to_string())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("The file changed while it was being read; try opening it again.".to_owned());
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{} is not a UTF-8 text file.", path.display()))?;
    Ok(DiskVersion {
        modified: after.modified().ok(),
        len: after.len(),
        text,
    })
}

fn resolve_target(path: &Path) -> Result<PathBuf, String> {
    // symlink_metadata also recognizes dangling links: do not replace them silently.
    if fs::symlink_metadata(path).is_ok() {
        return fs::canonicalize(path)
            .map_err(|e| format!("Cannot resolve {}: {e}", path.display()));
    }
    let name = path
        .file_name()
        .ok_or_else(|| "Choose a file name.".to_owned())?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent =
        fs::canonicalize(parent).map_err(|e| format!("Cannot access {}: {e}", parent.display()))?;
    Ok(parent.join(name))
}

fn atomic_write(
    target: &Path,
    bytes: &[u8],
    permissions: Option<Permissions>,
    before_replace: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "The file has no parent folder.".to_owned())?;
    let (temp, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".mde-{}-{sequence}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create a temporary file: {error}")),
        }
    };
    let result = (|| {
        file.write_all(bytes)
            .map_err(|e| format!("Cannot write the file: {e}"))?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)
                .map_err(|e| format!("Cannot preserve permissions: {e}"))?;
        }
        file.sync_all()
            .map_err(|e| format!("Cannot flush the file: {e}"))?;
        drop(file);
        before_replace()?;
        fs::rename(&temp, target)
            .map_err(|e| format!("Cannot replace {}: {e}", target.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub recent_files: Vec<PathBuf>,
    pub recent_folders: Vec<PathBuf>,
    pub dark: bool,
    pub font_size: f32,
    pub show_sidebar: bool,
    pub zen: bool,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent_files: Vec::new(),
            recent_folders: Vec::new(),
            dark: true,
            font_size: 16.0,
            show_sidebar: true,
            zen: true,
            theme: "catppuccin-mocha".into(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let mut settings: Self = fs::read(settings_path())
            .ok()
            .filter(|bytes| bytes.len() <= 1024 * 1024)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        settings.recent_files.truncate(RECENT_LIMIT);
        settings.recent_folders.truncate(RECENT_LIMIT);
        settings.font_size = if settings.font_size.is_finite() {
            settings.font_size.clamp(10.0, 32.0)
        } else {
            16.0
        };
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        let parent = path
            .parent()
            .ok_or_else(|| "Cannot locate the settings folder.".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create the settings folder: {e}"))?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(&path, &bytes, None, || Ok(()))
    }

    pub fn remember_file(&mut self, path: &Path) {
        remember(&mut self.recent_files, path);
    }

    pub fn remember_folder(&mut self, path: &Path) {
        remember(&mut self.recent_folders, path);
    }
}

fn remember(recent: &mut Vec<PathBuf>, path: &Path) {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    recent.retain(|previous| previous != &path);
    recent.insert(0, path);
    recent.truncate(RECENT_LIMIT);
}

fn settings_path() -> PathBuf {
    app_dir(false).join("settings.json")
}

fn app_dir(cache: bool) -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return home
                .join(if cache {
                    "Library/Caches"
                } else {
                    "Library/Application Support"
                })
                .join("mde");
        }
    } else if cfg!(target_os = "windows") {
        if let Some(base) = env::var_os(if cache { "LOCALAPPDATA" } else { "APPDATA" }) {
            return PathBuf::from(base).join("mde");
        }
    } else {
        if let Some(base) = env::var_os(if cache {
            "XDG_CACHE_HOME"
        } else {
            "XDG_CONFIG_HOME"
        }) && Path::new(&base).is_absolute()
        {
            return PathBuf::from(base).join("mde");
        }
        if let Some(home) = home {
            return home
                .join(if cache { ".cache" } else { ".config" })
                .join("mde");
        }
    }
    env::temp_dir().join("mde")
}

#[derive(Clone, Debug)]
pub struct FolderEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// List just one level. Subdirectories are read only when expanded by the UI.
pub fn read_folder(path: &Path) -> Result<Vec<FolderEntry>, String> {
    let entries = fs::read_dir(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Cannot read a folder entry: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        let is_dir = file_type.is_dir();
        let path = entry.path();
        let is_file = file_type.is_file()
            || (file_type.is_symlink()
                && fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()));
        if !is_dir && !(is_file && is_markdown(&path)) {
            continue;
        }
        result.push(FolderEntry { path, name, is_dir });
    }
    result.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(result)
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!("mde-test-{}-{sequence}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn file(&self, name: &str, text: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, text).unwrap();
            path
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn opens_utf8_and_tracks_changes_against_saved_text() {
        let dir = TempDir::new();
        let path = dir.file("hello.md", "# Привет\n".as_bytes());
        let mut document = Document::open(7, &path).unwrap();
        assert_eq!(document.id, 7);
        assert_eq!(document.title(), "hello.md");
        assert!(!document.is_dirty());
        document.text.push_str("new");
        assert!(document.is_dirty());
        document.text = document.saved_text.clone();
        assert!(!document.is_dirty());
        assert!(
            Document::open(1, &dir.file("bad.md", &[0xff]))
                .unwrap_err()
                .contains("UTF-8")
        );
    }

    #[test]
    fn saves_new_and_existing_documents_without_temp_artifacts() {
        let dir = TempDir::new();
        let target = dir.0.join("new.md");
        let mut document = Document::blank(1);
        document.text = "# First".to_owned();
        document.save(&target, false).unwrap();
        assert!(!document.is_dirty());
        assert_eq!(fs::read_to_string(&target).unwrap(), "# First");
        document.text = "# Second".to_owned();
        document.save(&target, false).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "# Second");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn detects_external_changes_even_when_length_is_unchanged() {
        let dir = TempDir::new();
        let path = dir.file("note.md", b"before");
        let mut document = Document::open(1, &path).unwrap();
        document.text = "editor".to_owned();
        fs::write(&path, b"remote").unwrap();
        assert!(document.save(&path, false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "remote");
        assert_eq!(document.saved_text, "before");
        assert!(document.is_dirty());
        document.save(&path, true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "editor");
    }

    #[test]
    fn accepts_background_save_without_losing_newer_edits() {
        let dir = TempDir::new();
        let path = dir.file("note.md", b"original");
        let mut document = Document::open(1, &path).unwrap();
        document.text = "first edit".to_owned();
        let mut snapshot = document.clone();
        document.text = "second edit".to_owned();
        snapshot.save(&path, false).unwrap();

        assert!(document.accept_saved_snapshot(snapshot));
        assert_eq!(document.text, "second edit");
        assert_eq!(document.saved_text, "first edit");
        assert!(document.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "first edit");
        // The accepted disk version must permit the next normal, conflict-checked save.
        document.save(&path, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second edit");
        assert!(!document.is_dirty());
    }

    #[test]
    fn rejects_background_snapshots_for_other_documents_or_unsaved_edits() {
        let dir = TempDir::new();
        let path = dir.file("note.md", b"original");
        let other_path = dir.file("other.md", b"other");
        let mut document = Document::open(1, &path).unwrap();
        document.text = "current edit".to_owned();
        let original_path = document.path.clone();

        let wrong_id = Document::open(2, &path).unwrap();
        assert!(!document.accept_saved_snapshot(wrong_id));
        let wrong_path = Document::open(1, &other_path).unwrap();
        assert!(!document.accept_saved_snapshot(wrong_path));
        let mut unsaved = Document::open(1, &path).unwrap();
        unsaved.text = "unsaved snapshot".to_owned();
        assert!(!document.accept_saved_snapshot(unsaved));
        assert_eq!(document.path, original_path);
        assert_eq!(document.text, "current edit");
        assert_eq!(document.saved_text, "original");
        // Rejected results must also preserve the original conflict baseline.
        document.save(&path, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "current edit");
        assert!(!Document::blank(3).accept_saved_snapshot(Document::blank(3)));
    }

    #[test]
    fn background_snapshot_preserves_external_conflict_protection() {
        let dir = TempDir::new();
        let path = dir.file("note.md", b"original");
        let mut document = Document::open(1, &path).unwrap();
        document.text = "first edit".to_owned();
        let mut snapshot = document.clone();
        document.text = "second edit".to_owned();
        fs::write(&path, b"external edit").unwrap();

        assert!(snapshot.save(&path, false).is_err());
        assert!(!document.accept_saved_snapshot(snapshot));
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
        assert_eq!(document.saved_text, "original");
        assert_eq!(document.text, "second edit");
        assert!(document.is_dirty());
        assert!(document.save(&path, false).is_err());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn save_as_requires_explicit_overwrite_and_missing_original_conflicts() {
        let dir = TempDir::new();
        let existing = dir.file("existing.md", b"keep");
        let mut document = Document::blank(1);
        document.text = "replace".to_owned();
        assert!(document.save(&existing, false).is_err());
        assert!(document.path.is_none());
        assert_eq!(fs::read_to_string(&existing).unwrap(), "keep");
        document.save(&existing, true).unwrap();
        fs::remove_file(&existing).unwrap();
        assert!(document.save(&existing, false).is_err());
        document.save(&existing, true).unwrap();
    }

    #[test]
    fn refuses_oversized_files_and_directories() {
        let dir = TempDir::new();
        let path = dir.0.join("large.md");
        File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        assert!(Document::open(1, &path).unwrap_err().contains("32 MiB"));
        assert!(Document::open(1, &dir.0).is_err());
    }

    #[test]
    fn scans_one_level_with_filtering_and_directories_first() {
        let dir = TempDir::new();
        for name in [
            "z.md",
            "A.MARKDOWN",
            "x.mdown",
            "y.mkd",
            "skip.txt",
            ".secret.md",
        ] {
            dir.file(name, b"");
        }
        for name in ["b-folder", "A-folder", "node_modules", "target", ".git"] {
            fs::create_dir(dir.0.join(name)).unwrap();
        }
        dir.file("b-folder/nested.md", b"");
        let entries = read_folder(&dir.0).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            [
                "A-folder",
                "b-folder",
                "A.MARKDOWN",
                "x.mdown",
                "y.mkd",
                "z.md"
            ]
        );
        assert!(entries[0].is_dir);
        assert!(!entries[2].is_dir);
    }

    #[test]
    fn recent_history_deduplicates_moves_to_front_and_is_bounded() {
        let dir = TempDir::new();
        let mut settings = Settings::default();
        for index in 0..25 {
            settings.remember_file(&dir.0.join(format!("{index}.md")));
        }
        settings.remember_file(&dir.0.join("10.md"));
        assert_eq!(settings.recent_files.len(), 20);
        assert_eq!(settings.recent_files[0], dir.0.join("10.md"));
        assert_eq!(
            settings
                .recent_files
                .iter()
                .filter(|path| path.ends_with("10.md"))
                .count(),
            1
        );
        settings.remember_folder(&dir.0);
        settings.remember_folder(&dir.0.join("."));
        assert_eq!(settings.recent_folders.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn saves_through_symlinks_preserving_links_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = TempDir::new();
        let original = dir.file("original.md", b"before");
        fs::set_permissions(&original, Permissions::from_mode(0o640)).unwrap();
        let link = dir.0.join("linked.md");
        symlink(&original, &link).unwrap();
        let mut document = Document::open(1, &link).unwrap();
        document.text = "after".to_owned();
        document.save(&link, false).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&original).unwrap(), "after");
        assert_eq!(
            fs::metadata(&original).unwrap().permissions().mode() & 0o777,
            0o640
        );
        symlink(&dir.0, dir.0.join("loop")).unwrap();
        assert!(
            !read_folder(&dir.0)
                .unwrap()
                .iter()
                .any(|entry| entry.name == "loop")
        );
    }

    #[test]
    fn failed_atomic_replacement_removes_temporary_file() {
        let dir = TempDir::new();
        let target = dir.0.join("never.md");
        assert!(atomic_write(&target, b"text", None, || Err("conflict".to_owned())).is_err());
        assert!(!target.exists());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
    }
}
