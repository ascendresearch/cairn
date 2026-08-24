use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

use cairn_protocol::{BlobDigest, ContentId, ContentType};
use cairn_record::{ContentDescriptor, ContentStore, ContentStoreError};
use rusqlite::{Connection, OptionalExtension, params};

use crate::schema;

/// Filesystem blob CAS with `SQLite` semantic metadata.
pub struct SqliteContentStore {
    connection: Connection,
    blob_root: PathBuf,
}

impl SqliteContentStore {
    /// Opens content metadata and creates the blob directory layout.
    ///
    /// # Errors
    ///
    /// Returns [`ContentStoreError`] when metadata or filesystem initialization fails.
    pub fn open(
        database_path: impl AsRef<Path>,
        blob_root: impl AsRef<Path>,
    ) -> Result<Self, ContentStoreError> {
        let mut connection = Connection::open(database_path).map_err(metadata_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(metadata_error)?;
        schema::migrate(&mut connection).map_err(metadata_error)?;

        let blob_root = blob_root.as_ref().to_path_buf();
        fs::create_dir_all(blob_root.join("objects/sha256")).map_err(io_error)?;
        fs::create_dir_all(blob_root.join("tmp")).map_err(io_error)?;
        Ok(Self {
            connection,
            blob_root,
        })
    }

    fn blob_path(&self, digest: BlobDigest) -> PathBuf {
        let hex = digest.hex();
        self.blob_root
            .join("objects/sha256")
            .join(&hex[..2])
            .join(&hex[2..])
    }

    fn publish_blob(
        &self,
        temporary: &tempfile::NamedTempFile,
        digest: BlobDigest,
        byte_len: u64,
    ) -> Result<(), ContentStoreError> {
        let target = self.blob_path(digest);
        let parent = target.parent().ok_or_else(|| ContentStoreError::Io {
            message: "blob target has no parent directory".to_owned(),
        })?;
        fs::create_dir_all(parent).map_err(io_error)?;
        match fs::hard_link(temporary.path(), &target) {
            Ok(()) => {
                File::open(parent)
                    .and_then(|directory| directory.sync_all())
                    .map_err(io_error)?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                verify_physical_blob(&target, digest, byte_len)
            }
            Err(error) => Err(io_error(error)),
        }
    }

    fn bind_metadata<T: ContentType>(
        &mut self,
        descriptor: &ContentDescriptor<T>,
    ) -> Result<(), ContentStoreError> {
        let transaction = self.connection.transaction().map_err(metadata_error)?;
        let blob_wire = descriptor.blob_digest.to_wire();
        let content_wire = descriptor.content_id.to_wire();
        let byte_len = to_i64(descriptor.byte_len)?;
        transaction
            .execute(
                "INSERT INTO content_blobs (blob_digest, byte_len) VALUES (?1, ?2)
                 ON CONFLICT(blob_digest) DO NOTHING",
                params![blob_wire, byte_len],
            )
            .map_err(metadata_error)?;
        let stored_blob_len = transaction
            .query_row(
                "SELECT byte_len FROM content_blobs WHERE blob_digest = ?1",
                [descriptor.blob_digest.to_wire()],
                |row| row.get::<_, i64>(0),
            )
            .map_err(metadata_error)?;
        if stored_blob_len != byte_len {
            return Err(ContentStoreError::Integrity {
                message: "blob digest is bound to a different byte length".to_owned(),
            });
        }

        transaction
            .execute(
                "INSERT INTO content_objects (
                    content_id, content_domain, algorithm, blob_digest, byte_len
                 ) VALUES (?1, ?2, 'sha256', ?3, ?4)
                 ON CONFLICT(content_id) DO NOTHING",
                params![content_wire, T::DOMAIN, blob_wire, byte_len],
            )
            .map_err(metadata_error)?;
        let stored = transaction
            .query_row(
                "SELECT content_domain, algorithm, blob_digest, byte_len
                 FROM content_objects WHERE content_id = ?1",
                [descriptor.content_id.to_wire()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .map_err(metadata_error)?;
        if stored
            != (
                T::DOMAIN.to_owned(),
                "sha256".to_owned(),
                blob_wire,
                byte_len,
            )
        {
            return Err(ContentStoreError::Integrity {
                message: "semantic content identity has conflicting metadata".to_owned(),
            });
        }
        transaction.commit().map_err(metadata_error)
    }
}

impl ContentStore for SqliteContentStore {
    fn put<T: ContentType>(
        &mut self,
        reader: &mut dyn Read,
    ) -> Result<ContentDescriptor<T>, ContentStoreError> {
        let mut temporary =
            tempfile::NamedTempFile::new_in(self.blob_root.join("tmp")).map_err(io_error)?;
        let byte_len = std::io::copy(reader, temporary.as_file_mut()).map_err(io_error)?;
        temporary.as_file().sync_all().map_err(io_error)?;

        temporary
            .as_file_mut()
            .seek(SeekFrom::Start(0))
            .map_err(io_error)?;
        let (blob_digest, observed_len) =
            BlobDigest::derive_reader(temporary.as_file_mut()).map_err(io_error)?;
        if observed_len != byte_len {
            return Err(ContentStoreError::Integrity {
                message: "temporary blob length changed while hashing".to_owned(),
            });
        }
        temporary
            .as_file_mut()
            .seek(SeekFrom::Start(0))
            .map_err(io_error)?;
        let content_id =
            ContentId::<T>::derive_reader(temporary.as_file_mut(), byte_len).map_err(|error| {
                ContentStoreError::Integrity {
                    message: error.to_string(),
                }
            })?;
        let descriptor = ContentDescriptor {
            content_id,
            blob_digest,
            byte_len,
        };
        self.publish_blob(&temporary, blob_digest, byte_len)?;
        self.bind_metadata(&descriptor)?;
        Ok(descriptor)
    }

    fn write_to<T: ContentType>(
        &self,
        content_id: &ContentId<T>,
        writer: &mut dyn Write,
    ) -> Result<ContentDescriptor<T>, ContentStoreError> {
        let metadata = self
            .connection
            .query_row(
                "SELECT blob_digest, byte_len FROM content_objects
                 WHERE content_id = ?1 AND content_domain = ?2 AND algorithm = 'sha256'",
                params![content_id.to_wire(), T::DOMAIN],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(metadata_error)?
            .ok_or_else(|| ContentStoreError::NotFound {
                content_id: content_id.to_wire(),
            })?;
        let blob_digest =
            BlobDigest::from_str(&metadata.0).map_err(|error| ContentStoreError::Integrity {
                message: format!("invalid stored blob digest: {error}"),
            })?;
        let byte_len = to_u64(metadata.1)?;
        let path = self.blob_path(blob_digest);
        let mut file = File::open(&path).map_err(io_error)?;
        let (actual_blob, actual_len) = BlobDigest::derive_reader(&mut file).map_err(io_error)?;
        if actual_blob != blob_digest || actual_len != byte_len {
            return Err(ContentStoreError::Integrity {
                message: "physical bytes do not match blob digest/length metadata".to_owned(),
            });
        }
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        let derived = ContentId::<T>::derive_reader(&mut file, byte_len).map_err(|error| {
            ContentStoreError::Integrity {
                message: error.to_string(),
            }
        })?;
        if derived != *content_id {
            return Err(ContentStoreError::Integrity {
                message: "physical bytes do not match semantic content identity".to_owned(),
            });
        }
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        let written = std::io::copy(&mut file, writer).map_err(io_error)?;
        if written != byte_len {
            return Err(ContentStoreError::Integrity {
                message: "physical byte length changed during read".to_owned(),
            });
        }
        Ok(ContentDescriptor {
            content_id: *content_id,
            blob_digest,
            byte_len,
        })
    }
}

fn verify_physical_blob(
    path: &Path,
    expected: BlobDigest,
    expected_len: u64,
) -> Result<(), ContentStoreError> {
    let mut file = File::open(path).map_err(io_error)?;
    let (actual, actual_len) = BlobDigest::derive_reader(&mut file).map_err(io_error)?;
    if actual != expected || actual_len != expected_len {
        return Err(ContentStoreError::Integrity {
            message: format!("blob {} failed digest/length verification", path.display()),
        });
    }
    Ok(())
}

fn to_i64(value: u64) -> Result<i64, ContentStoreError> {
    i64::try_from(value).map_err(|_| ContentStoreError::Metadata {
        message: "content byte length exceeds SQLite INTEGER".to_owned(),
    })
}

fn to_u64(value: i64) -> Result<u64, ContentStoreError> {
    u64::try_from(value).map_err(|_| ContentStoreError::Integrity {
        message: "stored content byte length is negative".to_owned(),
    })
}

fn io_error(error: impl std::fmt::Display) -> ContentStoreError {
    ContentStoreError::Io {
        message: error.to_string(),
    }
}

fn metadata_error(error: impl std::fmt::Display) -> ContentStoreError {
    ContentStoreError::Metadata {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use cairn_protocol::ContentType;
    use cairn_record::{ContentStore, ContentStoreError};

    use super::SqliteContentStore;

    struct SourceFile;
    impl ContentType for SourceFile {
        const DOMAIN: &'static str = "content.source-file.v1";
    }

    struct ModelInput;
    impl ContentType for ModelInput {
        const DOMAIN: &'static str = "content.model-input.v1";
    }

    #[test]
    fn same_bytes_deduplicate_physically_without_erasing_semantic_type() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut store = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("store");
        let bytes = b"kernel source";
        let source = store
            .put::<SourceFile>(&mut Cursor::new(bytes))
            .expect("source put");
        let input = store
            .put::<ModelInput>(&mut Cursor::new(bytes))
            .expect("input put");

        assert_ne!(source.content_id.to_wire(), input.content_id.to_wire());
        assert_eq!(source.blob_digest, input.blob_digest);
        let count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM content_blobs", [], |row| row.get(0))
            .expect("blob count");
        assert_eq!(count, 1);
    }

    #[test]
    fn verified_content_survives_store_reopen() {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("content.db");
        let cas = directory.path().join("cas");
        let descriptor = {
            let mut store = SqliteContentStore::open(&database, &cas).expect("store");
            store
                .put::<SourceFile>(&mut Cursor::new(b"persistent bytes"))
                .expect("put")
        };

        let store = SqliteContentStore::open(database, cas).expect("reopen");
        let mut output = Vec::new();
        let read = store
            .write_to(&descriptor.content_id, &mut output)
            .expect("verified read");
        assert_eq!(output, b"persistent bytes");
        assert_eq!(read.blob_digest, descriptor.blob_digest);
    }

    #[test]
    fn corrupted_blob_is_detected_before_output() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut store = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("store");
        let descriptor = store
            .put::<SourceFile>(&mut Cursor::new(b"trusted bytes"))
            .expect("put");
        std::fs::write(store.blob_path(descriptor.blob_digest), b"corrupt").expect("corrupt blob");

        let mut output = Vec::new();
        let error = store
            .write_to(&descriptor.content_id, &mut output)
            .expect_err("corruption must fail");
        assert!(matches!(error, ContentStoreError::Integrity { .. }));
        assert!(output.is_empty());
    }

    struct FailingReader {
        emitted: bool,
    }

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.emitted {
                return Err(std::io::Error::other("injected input failure"));
            }
            self.emitted = true;
            buffer[..4].copy_from_slice(b"half");
            Ok(4)
        }
    }

    #[test]
    fn failed_input_stream_creates_no_authoritative_metadata() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut store = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("store");
        let error = store
            .put::<SourceFile>(&mut FailingReader { emitted: false })
            .expect_err("read failure must fail");
        assert!(matches!(error, ContentStoreError::Io { .. }));
        let count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM content_objects", [], |row| row.get(0))
            .expect("object count");
        assert_eq!(count, 0);
    }
}
