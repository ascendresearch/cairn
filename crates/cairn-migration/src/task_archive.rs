//! Converting one submitted source archive into the task sources SIR reads.
//!
//! A migration task arrives as an archive because a real operator project is a directory tree, not
//! a handful of inline files. The archive is transport: what the system keeps is the per-path
//! source bundle, whose identity already pins the exact set of files and their exact bytes.
//!
//! Nothing here drops an entry it cannot represent. A submission converts completely or is
//! refused, which is what makes the resulting bundle a complete description of what was submitted;
//! silently trimming an archive would leave a record that agrees with no upload anyone made.

use std::io::Read;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::sir::SirTaskArtifactPath;

/// Bounds applied while a submitted archive is unpacked.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskArchiveLimits {
    /// Maximum bytes of the archive as received, checked before anything is decompressed.
    pub max_archive_bytes: u64,
    /// Maximum bytes produced by decompression, counted as it proceeds.
    pub max_uncompressed_bytes: u64,
    /// Maximum entries the archive may contain.
    pub max_entries: u32,
    /// Maximum bytes of any single entry.
    pub max_entry_bytes: u64,
}

impl Default for TaskArchiveLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 500 * 1024 * 1024,
            max_uncompressed_bytes: 1024 * 1024 * 1024,
            max_entries: 8192,
            max_entry_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Why a submitted archive cannot become task sources.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum TaskArchiveError {
    /// The archive is larger than the configured bound.
    #[error("submitted archive is {actual} bytes, above the {limit} byte limit")]
    ArchiveTooLarge {
        /// Bytes received.
        actual: u64,
        /// Configured bound.
        limit: u64,
    },
    /// Decompression produced more bytes than the configured bound allows.
    #[error("archive expands past the {0} byte decompression limit")]
    ExpansionTooLarge(u64),
    /// The archive holds more entries than the configured bound allows.
    #[error("archive holds more than {0} entries")]
    TooManyEntries(u32),
    /// One entry is larger than the configured bound.
    #[error("{path} is larger than the {limit} byte entry limit")]
    EntryTooLarge {
        /// Entry path.
        path: String,
        /// Configured bound.
        limit: u64,
    },
    /// The archive holds something other than a regular file or directory.
    #[error("{path} is not a regular file; links and device nodes are not accepted")]
    UnsupportedEntry {
        /// Entry path.
        path: String,
    },
    /// An entry names a path that would leave the task root.
    #[error("{path} is not a normalized relative path inside the task")]
    UnsafePath {
        /// Entry path as written in the archive.
        path: String,
    },
    /// An entry's bytes are not UTF-8, so it has no lines for intent to cite.
    #[error("{path} is not UTF-8; a task submission carries source, not binary material")]
    NotSource {
        /// Entry path.
        path: String,
    },
    /// Two entries name the same task path.
    #[error("{path} appears more than once")]
    DuplicatePath {
        /// The repeated path.
        path: String,
    },
    /// The archive holds no source at all.
    #[error("archive holds no source files")]
    Empty,
    /// The archive could not be read.
    #[error("archive could not be read: {0}")]
    Unreadable(String),
    /// An entry path is not a valid task artifact path.
    #[error("{path} is not a valid task path: {reason}")]
    InvalidPath {
        /// Entry path.
        path: String,
        /// Why it was refused.
        reason: String,
    },
}

/// Unpacks one gzip-compressed tar archive into sorted, unique task sources.
///
/// # Errors
///
/// Returns the first condition the archive fails. Nothing partial is returned: a submission
/// converts completely or not at all.
pub fn extract_task_sources(
    archive: &[u8],
    limits: TaskArchiveLimits,
) -> Result<Vec<(SirTaskArtifactPath, String)>, TaskArchiveError> {
    let received = archive.len() as u64;
    if received > limits.max_archive_bytes {
        return Err(TaskArchiveError::ArchiveTooLarge {
            actual: received,
            limit: limits.max_archive_bytes,
        });
    }
    // Counted while decompressing rather than after it. A bounded archive can expand without
    // bound, so checking the result would mean already holding whatever it expanded to.
    let decoder = BoundedRead {
        inner: GzDecoder::new(archive),
        remaining: limits.max_uncompressed_bytes,
    };
    let mut tar = tar::Archive::new(decoder);
    let mut sources: Vec<(SirTaskArtifactPath, String)> = Vec::new();
    let mut entries = 0_u32;
    for entry in tar
        .entries()
        .map_err(|error| unreadable(&error, limits.max_uncompressed_bytes))?
    {
        let mut entry = entry.map_err(|error| unreadable(&error, limits.max_uncompressed_bytes))?;
        entries = entries.saturating_add(1);
        if entries > limits.max_entries {
            return Err(TaskArchiveError::TooManyEntries(limits.max_entries));
        }
        let raw = entry
            .path()
            .map_err(|error| unreadable(&error, limits.max_uncompressed_bytes))?
            .to_string_lossy()
            .into_owned();
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err(TaskArchiveError::UnsupportedEntry { path: raw });
        }
        let size = entry.header().size().unwrap_or(u64::MAX);
        if size > limits.max_entry_bytes {
            return Err(TaskArchiveError::EntryTooLarge {
                path: raw,
                limit: limits.max_entry_bytes,
            });
        }
        let normalized =
            normalize(&raw).ok_or_else(|| TaskArchiveError::UnsafePath { path: raw.clone() })?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| unreadable(&error, limits.max_uncompressed_bytes))?;
        let text = String::from_utf8(bytes).map_err(|_| TaskArchiveError::NotSource {
            path: normalized.clone(),
        })?;
        let path = SirTaskArtifactPath::new(normalized.clone()).map_err(|error| {
            TaskArchiveError::InvalidPath {
                path: normalized,
                reason: error.to_string(),
            }
        })?;
        sources.push((path, text));
    }
    if sources.is_empty() {
        return Err(TaskArchiveError::Empty);
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    if let Some(pair) = sources.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(TaskArchiveError::DuplicatePath {
            path: pair[0].0.as_str().to_owned(),
        });
    }
    Ok(sources)
}

fn unreadable(error: &std::io::Error, limit: u64) -> TaskArchiveError {
    if error.kind() == std::io::ErrorKind::InvalidData
        && error.to_string().contains(EXPANSION_MARKER)
    {
        return TaskArchiveError::ExpansionTooLarge(limit);
    }
    TaskArchiveError::Unreadable(error.to_string())
}

const EXPANSION_MARKER: &str = "cairn: decompression limit reached";

/// Accepts one relative path with no parent or current component, tolerating the leading `./` that
/// `tar -C dir -cf - .` writes, which spells the same path a different way.
fn normalize(raw: &str) -> Option<String> {
    let trimmed = raw.strip_prefix("./").unwrap_or(raw);
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.contains('\\')
        || trimmed
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }
    Some(trimmed.to_owned())
}

struct BoundedRead<R: Read> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for BoundedRead<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                EXPANSION_MARKER,
            ));
        }
        let ceiling = usize::try_from(self.remaining).unwrap_or(usize::MAX);
        let window = buffer.len().min(ceiling);
        let read = self.inner.read(&mut buffer[..window])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use flate2::{Compression, write::GzEncoder};

    use super::{TaskArchiveError, TaskArchiveLimits, extract_task_sources, normalize};

    enum Entry<'a> {
        File(&'a str, &'a [u8]),
        Directory(&'a str),
    }

    fn archive(entries: &[Entry<'_>]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for entry in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            match entry {
                Entry::File(path, bytes) => {
                    header.set_size(bytes.len() as u64);
                    header.set_entry_type(tar::EntryType::Regular);
                    header.set_cksum();
                    builder
                        .append_data(&mut header, path, *bytes)
                        .expect("append file");
                }
                Entry::Directory(path) => {
                    header.set_size(0);
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_cksum();
                    builder
                        .append_data(&mut header, path, &[][..])
                        .expect("append directory");
                }
            }
        }
        builder
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip")
    }

    fn limits() -> TaskArchiveLimits {
        TaskArchiveLimits::default()
    }

    #[test]
    fn a_source_tree_converts_to_sorted_unique_task_sources() {
        let bytes = archive(&[
            Entry::File("src/kernel.cu", b"__global__ void k() {}\n"),
            Entry::File("CMakeLists.txt", b"project(reduce)\n"),
            Entry::Directory("include/"),
        ]);
        let sources = extract_task_sources(&bytes, limits()).expect("converts");
        let paths: Vec<&str> = sources.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths, ["CMakeLists.txt", "src/kernel.cu"]);
        assert_eq!(sources[1].1, "__global__ void k() {}\n");
    }

    // `tar -C dir -cf - .` writes every path with a leading `./`, which spells the same path a
    // different way rather than a different path.
    #[test]
    fn a_leading_current_directory_is_the_same_path() {
        let bytes = archive(&[Entry::File("./src/kernel.cu", b"x\n")]);
        let sources = extract_task_sources(&bytes, limits()).expect("converts");
        assert_eq!(sources[0].0.as_str(), "src/kernel.cu");
    }

    // Tested on the function that decides it, because the archive writer refuses to produce some
    // of these and a reader must still refuse them: an archive can be written by anything.
    #[test]
    fn only_a_normalized_relative_path_inside_the_task_is_accepted() {
        assert_eq!(normalize("src/kernel.cu").as_deref(), Some("src/kernel.cu"));
        assert_eq!(
            normalize("./src/kernel.cu").as_deref(),
            Some("src/kernel.cu")
        );
        for path in [
            "../outside.cu",
            "/etc/passwd",
            "src/../../escape.cu",
            "src/./kernel.cu",
            "src//kernel.cu",
            "src/",
            "src\\kernel.cu",
            "",
            ".",
        ] {
            assert!(normalize(path).is_none(), "{path:?} must not be accepted");
        }
    }

    // Binary material has no lines, and intent claims cite lines. Refusing it names the file
    // instead of quietly dropping it, so a submission and the bundle made from it always agree.
    #[test]
    fn material_that_is_not_source_is_refused_by_name() {
        let bytes = archive(&[Entry::File("data/weights.bin", &[0xff, 0xfe, 0x00, 0x01])]);
        assert_eq!(
            extract_task_sources(&bytes, limits()),
            Err(TaskArchiveError::NotSource {
                path: "data/weights.bin".to_owned()
            })
        );
    }

    #[test]
    fn an_archive_above_the_limit_is_refused_before_it_is_opened() {
        let bytes = archive(&[Entry::File("a.cu", b"x\n")]);
        let limits = TaskArchiveLimits {
            max_archive_bytes: 8,
            ..TaskArchiveLimits::default()
        };
        assert!(matches!(
            extract_task_sources(&bytes, limits),
            Err(TaskArchiveError::ArchiveTooLarge { limit: 8, .. })
        ));
    }

    // The bound that matters: a small archive can expand without bound, so expansion is counted
    // while it happens. Checking the result would mean already holding whatever it produced.
    #[test]
    fn an_archive_that_expands_past_the_limit_is_stopped_while_expanding() {
        let payload = vec![b'x'; 4 * 1024 * 1024];
        let bytes = archive(&[Entry::File("big.cu", &payload)]);
        assert!(
            bytes.len() < 64 * 1024,
            "the fixture must compress far below what it expands to"
        );
        let limits = TaskArchiveLimits {
            max_uncompressed_bytes: 64 * 1024,
            ..TaskArchiveLimits::default()
        };
        // Matched rather than compared: a failure here would otherwise print the whole expansion.
        assert!(matches!(
            extract_task_sources(&bytes, limits),
            Err(TaskArchiveError::ExpansionTooLarge(65_536))
        ));
    }

    #[test]
    fn an_oversized_entry_is_refused() {
        let payload = vec![b'x'; 4096];
        let bytes = archive(&[Entry::File("big.cu", &payload)]);
        let limits = TaskArchiveLimits {
            max_entry_bytes: 1024,
            ..TaskArchiveLimits::default()
        };
        assert!(matches!(
            extract_task_sources(&bytes, limits),
            Err(TaskArchiveError::EntryTooLarge { limit: 1024, .. })
        ));
    }

    #[test]
    fn an_archive_with_no_source_is_refused() {
        let bytes = archive(&[Entry::Directory("empty/")]);
        assert_eq!(
            extract_task_sources(&bytes, limits()),
            Err(TaskArchiveError::Empty)
        );
    }
}
