//! Loading and saving documents.
//!
//! Saves are atomic: the new bytes go to a sibling temporary file which is
//! `fsync`ed and then renamed over the target, so a crash mid-write can never
//! leave a half-written document behind.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::files::encoding::{self, EncodingInfo};
use crate::files::line_endings::LineEnding;

/// A file read from disk, fully decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedFile {
    pub path: PathBuf,
    pub content: String,
    pub encoding: String,
    pub line_ending: LineEnding,
    pub had_bom: bool,
    pub byte_size: usize,
}

/// Largest file the editor will open (16 MiB of text). Beyond that the
/// per-line model would be unusable, so we refuse with a clear message.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Read and decode a text file.
pub fn load_file(path: &Path) -> Result<LoadedFile> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("cannot stat {}", path.display()))?;
    if metadata.is_dir() {
        bail!("{} is a directory", path.display());
    }
    if metadata.len() > MAX_FILE_BYTES {
        bail!(
            "{} is {} MiB, above the {} MiB limit",
            path.display(),
            metadata.len() / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        );
    }

    let raw = fs::read(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let info: EncodingInfo = encoding::detect_encoding(&raw);
    let text = encoding::decode(&raw, &info);
    let line_ending = LineEnding::detect(&text);

    // Normalise to LF in memory; the original style is restored on save.
    let content = LineEnding::Lf.apply(&text);

    Ok(LoadedFile {
        path: path.to_path_buf(),
        content,
        encoding: info.label,
        line_ending,
        had_bom: info.has_bom,
        byte_size: raw.len(),
    })
}

/// Encode `content` and write it atomically to `path`.
pub fn save_file(
    path: &Path,
    content: &str,
    encoding_label: &str,
    line_ending: LineEnding,
) -> Result<()> {
    save_file_with_bom(path, content, encoding_label, line_ending, false)
}

/// As [`save_file`] but with explicit control over a UTF-8 BOM.
pub fn save_file_with_bom(
    path: &Path,
    content: &str,
    encoding_label: &str,
    line_ending: LineEnding,
    with_bom: bool,
) -> Result<()> {
    let normalised = line_ending.apply(content);
    let bytes = encoding::encode(&normalised, encoding_label, with_bom);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }

    let tmp = temp_sibling(path);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .with_context(|| format!("cannot create temp file {}", tmp.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        file.sync_all().ok();
    }

    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err).with_context(|| format!("cannot rename to {}", path.display()))
        }
    }
}

/// Append to a file, creating it when missing.
pub fn append_file(path: &Path, content: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .with_context(|| format!("cannot open {} for append", path.display()))?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// `true` when the path exists and is a regular file.
pub fn file_exists(path: &str) -> bool {
    Path::new(path)
        .metadata()
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// Display name used for tab titles.
pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Ensure the path carries `extension`, adding it when missing.
pub fn with_extension(path: &Path, extension: &str) -> PathBuf {
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        path.with_extension(extension)
    }
}

/// A unique hidden sibling used as the write target.
fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let unique = format!(
        ".{}.{:x}.tmp",
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(unique),
        _ => PathBuf::from(unique),
    }
}

/// Open a file handle for reading; used by the "reveal in file manager" path.
pub fn open_handle(path: &Path) -> Result<File> {
    File::open(path).with_context(|| format!("cannot open {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn saves_and_loads_plain_utf8() {
        let dir = tmp_dir();
        let path = dir.path().join("a.txt");
        save_file(&path, "hello\nworld", "utf-8", LineEnding::Lf).unwrap();
        let loaded = load_file(&path).unwrap();
        assert_eq!(loaded.content, "hello\nworld");
        assert_eq!(loaded.encoding, "utf-8");
        assert_eq!(loaded.line_ending, LineEnding::Lf);
        assert!(!loaded.had_bom);
    }

    #[test]
    fn save_applies_the_requested_line_ending() {
        let dir = tmp_dir();
        let path = dir.path().join("win.txt");
        save_file(&path, "a\nb", "utf-8", LineEnding::Crlf).unwrap();
        let raw = fs::read(&path).unwrap();
        assert_eq!(raw, b"a\r\nb");
        let loaded = load_file(&path).unwrap();
        assert_eq!(loaded.line_ending, LineEnding::Crlf);
        assert_eq!(loaded.content, "a\nb", "normalised back to LF in memory");
    }

    #[test]
    fn save_can_write_a_utf8_bom() {
        let dir = tmp_dir();
        let path = dir.path().join("bom.txt");
        save_file_with_bom(&path, "hi", "utf-8", LineEnding::Lf, true).unwrap();
        let raw = fs::read(&path).unwrap();
        assert!(raw.starts_with(&[0xEF, 0xBB, 0xBF]));
        let loaded = load_file(&path).unwrap();
        assert!(loaded.had_bom);
        assert_eq!(loaded.content, "hi");
    }

    #[test]
    fn save_roundtrips_utf16() {
        let dir = tmp_dir();
        let path = dir.path().join("u16.txt");
        save_file(&path, "héllo", "utf-16le", LineEnding::Lf).unwrap();
        let loaded = load_file(&path).unwrap();
        assert_eq!(loaded.content, "héllo");
        assert_eq!(loaded.encoding, "utf-16le");
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tmp_dir();
        let path = dir.path().join("nested/deep/a.txt");
        save_file(&path, "x", "utf-8", LineEnding::Lf).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tmp_dir();
        let path = dir.path().join("clean.txt");
        save_file(&path, "x", "utf-8", LineEnding::Lf).unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "found {leftovers:?}");
    }

    #[test]
    fn saving_over_an_existing_file_replaces_it() {
        let dir = tmp_dir();
        let path = dir.path().join("over.txt");
        save_file(&path, "first", "utf-8", LineEnding::Lf).unwrap();
        save_file(&path, "second", "utf-8", LineEnding::Lf).unwrap();
        assert_eq!(load_file(&path).unwrap().content, "second");
    }

    #[test]
    fn loading_a_missing_file_reports_the_path() {
        let err = load_file(Path::new("/definitely/not/here.txt")).unwrap_err();
        assert!(err.to_string().contains("not/here.txt"));
    }

    #[test]
    fn loading_a_directory_is_rejected() {
        let dir = tmp_dir();
        let err = load_file(dir.path()).unwrap_err();
        assert!(err.to_string().contains("directory"));
    }

    #[test]
    fn loading_an_empty_file_yields_one_blank_line() {
        let dir = tmp_dir();
        let path = dir.path().join("empty.txt");
        fs::write(&path, b"").unwrap();
        let loaded = load_file(&path).unwrap();
        assert_eq!(loaded.content, "");
        assert_eq!(loaded.byte_size, 0);
    }

    #[test]
    fn file_exists_distinguishes_files_from_directories() {
        let dir = tmp_dir();
        let path = dir.path().join("f.txt");
        fs::write(&path, b"x").unwrap();
        assert!(file_exists(path.to_str().unwrap()));
        assert!(!file_exists(dir.path().to_str().unwrap()));
        assert!(!file_exists("/nope/nope.txt"));
    }

    #[test]
    fn file_name_extracts_the_last_component() {
        assert_eq!(file_name("/tmp/a/b/notes.txt"), "notes.txt");
        assert_eq!(file_name("notes.txt"), "notes.txt");
    }

    #[test]
    fn with_extension_only_adds_when_missing() {
        let added = with_extension(Path::new("/tmp/notes"), "npro");
        assert_eq!(added.file_name().unwrap(), "notes.npro");
        let kept = with_extension(Path::new("/tmp/notes.txt"), "npro");
        assert_eq!(kept.file_name().unwrap(), "notes.txt");
    }

    #[test]
    fn temp_sibling_is_hidden_and_unique() {
        let a = temp_sibling(Path::new("/tmp/notes.txt"));
        let b = temp_sibling(Path::new("/tmp/notes.txt"));
        assert!(a.file_name().unwrap().to_string_lossy().starts_with('.'));
        assert!(a.to_string_lossy().ends_with(".tmp"));
        // Sibling check, expressed without a platform-specific prefix.
        assert_eq!(a.parent(), Path::new("/tmp"), "the temp file must sit next to the original");
        assert_ne!(a, b, "consecutive temps must not collide");
    }

    #[test]
    fn temp_sibling_handles_a_bare_file_name() {
        let t = temp_sibling(Path::new("notes.txt"));
        assert!(t.to_string_lossy().ends_with(".tmp"));
        assert!(t.parent().unwrap().as_os_str().is_empty());
    }

    #[test]
    fn append_file_adds_to_the_end() {
        let dir = tmp_dir();
        let path = dir.path().join("log.txt");
        append_file(&path, "one\n").unwrap();
        append_file(&path, "two\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
    }

    #[test]
    fn open_handle_reads_the_file() {
        let dir = tmp_dir();
        let path = dir.path().join("h.txt");
        fs::write(&path, b"abc").unwrap();
        let mut handle = open_handle(&path).unwrap();
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut handle, &mut buf).unwrap();
        assert_eq!(buf, "abc");
    }

    #[test]
    fn large_files_are_refused_with_a_clear_message() {
        let dir = tmp_dir();
        let path = dir.path().join("big.txt");
        // Sparse-write a file just over the limit without allocating it.
        let file = File::create(&path).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        let err = load_file(&path).unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }
}
