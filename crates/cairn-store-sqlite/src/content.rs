use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

use cairn_protocol::{BlobDigest, ContentId, ContentType};
use cairn_record::{ContentDescriptor, ContentRangeStore, ContentStore, ContentStoreError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

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
        schema::initialize(&mut connection).map_err(metadata_error)?;

        let blob_root = blob_root.as_ref().to_path_buf();
        fs::create_dir_all(blob_root.join("objects/sha256")).map_err(io_error)?;
        fs::create_dir_all(blob_root.join("tmp")).map_err(io_error)?;
        Ok(Self {
            connection,
            blob_root,
        })
    }

    /// Opens a frozen content-store snapshot without schema, metadata, or filesystem mutation.
    ///
    /// The caller must guarantee that the database will not change for this handle's lifetime.
    /// `SQLite` immutable mode intentionally does not coordinate with a concurrent WAL writer.
    ///
    /// # Errors
    ///
    /// Returns an error when the database/CAS does not exist, is not current V1, or cannot be
    /// opened read-only.
    pub fn open_immutable_read_only(
        database_path: impl AsRef<Path>,
        blob_root: impl AsRef<Path>,
    ) -> Result<Self, ContentStoreError> {
        let database_uri = immutable_database_uri(database_path.as_ref())?;
        let connection = Connection::open_with_flags(
            database_uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(metadata_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(metadata_error)?;
        schema::validate_read_only(&connection).map_err(|error| ContentStoreError::Metadata {
            message: error.to_string(),
        })?;
        let blob_root = blob_root.as_ref().to_path_buf();
        if !blob_root.join("objects/sha256").is_dir() {
            return Err(ContentStoreError::Io {
                message: "read-only CAS object root does not exist".to_owned(),
            });
        }
        Ok(Self {
            connection,
            blob_root,
        })
    }

    /// Opens a read-only store that coordinates with a concurrent WAL writer.
    ///
    /// Unlike [`Self::open_immutable_read_only`], this mode observes committed WAL revisions and is
    /// therefore the correct boundary for an independently sandboxed reader consuming a live
    /// Controller content store. The `SQLite` handle has no write capability.
    ///
    /// # Errors
    ///
    /// Returns an error when the database/CAS does not exist, is not current V1, or cannot be
    /// opened read-only.
    pub fn open_read_only(
        database_path: impl AsRef<Path>,
        blob_root: impl AsRef<Path>,
    ) -> Result<Self, ContentStoreError> {
        let connection =
            Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(metadata_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(metadata_error)?;
        schema::validate_read_only(&connection).map_err(|error| ContentStoreError::Metadata {
            message: error.to_string(),
        })?;
        let blob_root = blob_root.as_ref().to_path_buf();
        if !blob_root.join("objects/sha256").is_dir() {
            return Err(ContentStoreError::Io {
                message: "read-only CAS object root does not exist".to_owned(),
            });
        }
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

fn immutable_database_uri(path: &Path) -> Result<String, ContentStoreError> {
    let path = fs::canonicalize(path).map_err(io_error)?;
    let path = path.to_str().ok_or_else(|| ContentStoreError::Io {
        message: "read-only SQLite path must be UTF-8".to_owned(),
    })?;
    let mut uri = String::from("file:");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            uri.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(&mut uri, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    Ok(uri)
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

impl ContentRangeStore for SqliteContentStore {
    fn write_range_to<T: ContentType>(
        &self,
        content_id: &ContentId<T>,
        offset: u64,
        range_byte_len: u64,
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
        let total_byte_len = to_u64(metadata.1)?;
        let end = offset
            .checked_add(range_byte_len)
            .filter(|end| *end <= total_byte_len)
            .ok_or_else(|| ContentStoreError::Integrity {
                message: "requested content range is outside the immutable object".to_owned(),
            })?;
        let path = self.blob_path(blob_digest);
        let mut file = File::open(&path).map_err(io_error)?;
        if file.metadata().map_err(io_error)?.len() != total_byte_len {
            return Err(ContentStoreError::Integrity {
                message: "physical byte length differs from immutable metadata".to_owned(),
            });
        }
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        let written = std::io::copy(&mut file.take(end - offset), writer).map_err(io_error)?;
        if written != range_byte_len {
            return Err(ContentStoreError::Integrity {
                message: "physical content range ended before its declared length".to_owned(),
            });
        }
        Ok(ContentDescriptor {
            content_id: *content_id,
            blob_digest,
            byte_len: total_byte_len,
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
    use cairn_record::{ContentRangeStore, ContentStore, ContentStoreError};

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
    fn read_only_store_reads_existing_content_and_cannot_publish() {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("content.db");
        let cas = directory.path().join("cas");
        let descriptor = {
            let mut store = SqliteContentStore::open(&database, &cas).expect("store");
            store
                .put::<SourceFile>(&mut Cursor::new(b"public immutable bytes"))
                .expect("put")
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o444))
                .expect("read-only database mode");
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o555))
                .expect("read-only parent mode");
        }
        let mut store =
            SqliteContentStore::open_immutable_read_only(&database, &cas).expect("read-only");
        let mut output = Vec::new();
        store
            .write_to(&descriptor.content_id, &mut output)
            .expect("read existing");
        assert_eq!(output, b"public immutable bytes");
        assert!(
            store
                .put::<SourceFile>(&mut Cursor::new(b"forbidden write"))
                .is_err()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))
                .expect("restore temporary directory mode");
        }
    }

    #[test]
    fn coordinated_read_only_store_observes_committed_live_wal_content() {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("content.db");
        let cas = directory.path().join("cas");
        let mut writer = SqliteContentStore::open(&database, &cas).expect("writer");
        let descriptor = writer
            .put::<SourceFile>(&mut Cursor::new(b"committed live bytes"))
            .expect("put");
        assert!(database.with_extension("db-wal").is_file());

        let mut reader = SqliteContentStore::open_read_only(&database, &cas).expect("reader");
        let mut output = Vec::new();
        reader
            .write_to(&descriptor.content_id, &mut output)
            .expect("read committed WAL content");
        assert_eq!(output, b"committed live bytes");
        assert!(
            reader
                .put::<SourceFile>(&mut Cursor::new(b"forbidden write"))
                .is_err()
        );
    }

    #[test]
    fn range_source_reads_exact_offsets_and_rejects_overrun() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut store = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("store");
        let descriptor = store
            .put::<SourceFile>(&mut Cursor::new(b"0123456789"))
            .expect("put");
        let mut range = Vec::new();
        let observed = store
            .write_range_to(&descriptor.content_id, 3, 4, &mut range)
            .expect("range");
        assert_eq!(range, b"3456");
        assert_eq!(observed.content_id, descriptor.content_id);
        assert_eq!(observed.blob_digest, descriptor.blob_digest);
        assert_eq!(observed.byte_len, descriptor.byte_len);
        assert!(matches!(
            store.write_range_to(&descriptor.content_id, 9, 2, &mut Vec::new()),
            Err(ContentStoreError::Integrity { .. })
        ));
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
