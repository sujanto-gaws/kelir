//! Where an attachment's bytes actually go (FR-ATT-001, [#244]).
//!
//! # One object store, built once, held on the state
//!
//! [`crate::mail::Mailer`]'s shape, for its reasons: built from configuration at
//! startup so a misconfiguration is loud on the first line of the log rather
//! than on somebody's first upload, held on [`crate::state::AppState`] so every
//! request shares one connection pool, and constructible directly so a test can
//! hand the router the store it will then read from.
//!
//! # An unconfigured deployment boots, and says so every time it is asked
//!
//! [`ObjectStorage::Unavailable`] is the mailer's `Logged` variant one module
//! over. A deployment with no object storage still starts, still serves every
//! other route, and refuses uploads with a message naming the configuration —
//! because attachments arriving in Phase 6 must not stop a Phase 5 deployment
//! from running. What it does **not** do is fall back to somewhere the bytes
//! would appear to be stored: an in-memory store would make an upload succeed
//! and the file vanish, which is worse than the refusal by exactly the amount a
//! person trusts a success.
//!
//! # The bucket is the deployment's, not this process's
//!
//! Nothing here creates one. The credentials this process holds should be able
//! to put and get objects, and abort an upload of its own, in a bucket somebody
//! else provisioned; a process that can create buckets can create the one an
//! attacker names. `deploy/docker` creates it, and CI's backend job creates it
//! before the tests run.
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use futures_util::stream::{BoxStream, StreamExt, TryStreamExt};
use object_store::aws::{AmazonS3Builder, AmazonS3ConfigKey};
use object_store::path::Path as ObjectPath;
use object_store::{ClientOptions, MultipartUpload, ObjectStore, PutPayload};

use crate::config::AppConfig;
use crate::error::AppError;

/// `x-amz-server-side-encryption`'s value for SSE-S3: the store encrypts under
/// its own key (`MINIO_KMS_SECRET_KEY`).
const SSE_S3: &str = "AES256";

/// How long one request to the store may last, from connecting to its last byte.
///
/// **`object_store`'s default is 30 seconds, and it counts the response body.**
/// While a download was read into memory that was the store's own speed. Now the
/// body is read as the person taking it reads, so the store's response lasts as
/// long as their connection does: past 30 seconds the read fails and is resumed,
/// past `RetryConfig`'s three minutes it is not, and the transfer breaks. Measured
/// [#834] against MinIO: at the default, a reader that paused for 200 seconds
/// stopped at 7,577 bytes of 25 MiB. At this value, a reader taking 240 seconds
/// over the file finished it.
///
/// **A reader that stops altogether can still lose the transfer**: paused for 200
/// seconds at this value, it stopped at 8 MiB, past what the sockets held. That
/// is the truncation [#834] AC10 describes, and the person can ask again.
///
/// This is a ceiling on how slowly a file can be taken, and it is what a store
/// that hangs can cost one request. The connect timeout stays at its 5 seconds.
///
/// [#834]: https://github.com/sujanto-gaws/kelir/issues/834
const STORE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How much of a file goes to the store at a time, and the most an upload holds.
///
/// **The smallest S3 accepts for every part but the last**, and exactly this for
/// each of them, because some S3-compatible stores also ask that they be equal.
/// A file no longer than this is one ordinary `put`.
pub const PART_SIZE: usize = 5 * 1024 * 1024;

/// An object's bytes as the store sends them, for a reader to pull ([#834]).
///
/// [#834]: https://github.com/sujanto-gaws/kelir/issues/834
pub type ObjectStream = BoxStream<'static, Result<Bytes, object_store::Error>>;

/// One object, found and not yet read.
///
/// **That the store answered is all this says.** A failure while the bytes are
/// read comes out of `stream` as an `Err` and has already been logged, so a
/// reader's job is only to stop: a download truncates, and a scan asks nothing.
pub struct StoredObject {
    /// As the store reports it, which is what a download's `Content-Length` says.
    pub size: u64,
    pub stream: ObjectStream,
}

/// The object store this process writes attachments to.
#[derive(Clone)]
pub enum ObjectStorage {
    /// An S3-compatible endpoint — MinIO in every deployment this project has.
    S3 {
        store: Arc<dyn ObjectStore>,
        bucket: String,
    },
    /// No usable configuration. Boots, refuses uploads, names the reason.
    Unavailable { reason: String },
}

impl std::fmt::Debug for ObjectStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::S3 { bucket, .. } => formatter
                .debug_struct("ObjectStorage::S3")
                .field("bucket", bucket)
                .finish_non_exhaustive(),
            Self::Unavailable { reason } => formatter
                .debug_struct("ObjectStorage::Unavailable")
                .field("reason", reason)
                .finish(),
        }
    }
}

impl ObjectStorage {
    /// Opens one object for reading.
    ///
    /// **A missing object is an internal error, not a 404.** The caller reached
    /// here by naming a row that exists and that they may read; if the bytes it
    /// points at are gone, the product is inconsistent and saying *not found*
    /// would report that as the caller's mistake. [`super::service::upload`]
    /// writes the object before the row precisely so this cannot happen, so
    /// reaching it means something outside this service removed an object.
    pub async fn get(&self, reference: &str) -> Result<StoredObject, AppError> {
        match self {
            Self::S3 { store, bucket } => {
                let path = ObjectPath::from(reference);

                let object = store.get(&path).await.map_err(|error| {
                    tracing::error!(
                        %error,
                        %bucket,
                        %reference,
                        "an attachment's row exists and its object could not be read"
                    );

                    AppError::Internal {
                        source: anyhow::anyhow!("object storage could not serve the file: {error}"),
                    }
                })?;

                let size = object.meta.size;
                let (bucket, reference) = (bucket.clone(), reference.to_owned());
                let stream = object
                    .into_stream()
                    .inspect_err(move |error| {
                        tracing::error!(%error, %bucket, %reference, "an attachment's bytes stopped mid-read");
                    })
                    .boxed();

                Ok(StoredObject { size, stream })
            }
            Self::Unavailable { reason } => Err(AppError::Internal {
                source: anyhow::anyhow!(
                    "this deployment has no object storage configured, so a file cannot be \
                     served: {reason}"
                ),
            }),
        }
    }

    /// Builds the store the configuration describes.
    ///
    /// **Builds rather than connects.** `AmazonS3Builder::build` validates the
    /// endpoint, the bucket name and the credentials' shape and opens nothing,
    /// so a wrong password is not discovered here — it is discovered on the
    /// first put, which is where the error names it. What this catches is the
    /// configuration that could never work.
    pub fn from_config(config: &AppConfig) -> Self {
        // `with_allow_http` because MinIO in the compose stack speaks plain
        // HTTP on 9000 and a deployment that terminates TLS elsewhere is the
        // ordinary case. A production endpoint is an `https://` URL and this
        // flag then changes nothing.
        //
        // **Every write asks for SSE-S3, and nothing turns that off**
        // (NFR-SEC-009, ADR-0049 §2). A bucket default is a property of the
        // deployment nothing here can see; the request is what makes a store
        // without a key refuse the write, so an upload fails where somebody
        // sees it rather than landing in plaintext. Set on the store, so it
        // covers `PutObject`, `CreateMultipartUpload` and `CopyObject` alike.
        // `S3EncryptionConfigKey` is not exported by `object_store` 0.12, so
        // the key is named by its string.
        let built = "aws_server_side_encryption"
            .parse::<AmazonS3ConfigKey>()
            .and_then(|server_side_encryption| {
                Self::builder(config)
                    .with_config(server_side_encryption, SSE_S3)
                    .build()
            });

        match built {
            Ok(store) => Self::S3 {
                store: Arc::new(store),
                bucket: config.storage_bucket.clone(),
            },
            Err(error) => {
                tracing::error!(
                    %error,
                    endpoint = %config.storage_endpoint,
                    bucket = %config.storage_bucket,
                    "object storage is not configured; attachment uploads will be refused"
                );

                Self::Unavailable {
                    reason: error.to_string(),
                }
            }
        }
    }

    fn builder(config: &AppConfig) -> AmazonS3Builder {
        // The client options first: they replace what `with_allow_http` sets.
        AmazonS3Builder::new()
            .with_client_options(ClientOptions::new().with_timeout(STORE_REQUEST_TIMEOUT))
            .with_endpoint(&config.storage_endpoint)
            .with_bucket_name(&config.storage_bucket)
            .with_access_key_id(&config.storage_access_key)
            .with_secret_access_key(&config.storage_secret_key)
            .with_region(&config.storage_region)
            .with_allow_http(true)
            .with_virtual_hosted_style_request(false)
    }

    /// Writes one object whole, which is how a file of one part or less is
    /// stored. [`Self::begin_upload`] is the other way in.
    ///
    /// **The failure this returns leaves nothing behind that a reader can
    /// see.** Its caller writes the row only after this succeeds, which is
    /// [#244] AC2's decision: an object with no row costs storage and reaches
    /// nobody, and a row whose `storage_reference` points at nothing is a
    /// download that answers 500 to somebody who did nothing wrong.
    pub async fn put(&self, reference: &str, bytes: Bytes) -> Result<(), AppError> {
        match self {
            Self::S3 { store, bucket } => {
                let path = ObjectPath::from(reference);

                store
                    .put(&path, PutPayload::from_bytes(bytes))
                    .await
                    .map_err(|error| {
                        tracing::error!(
                            %error,
                            %bucket,
                            %reference,
                            "an attachment's bytes could not be stored"
                        );

                        AppError::Internal {
                            source: anyhow::anyhow!("object storage refused the write: {error}"),
                        }
                    })?;

                Ok(())
            }
            Self::Unavailable { reason } => Err(no_store(reason)),
        }
    }

    /// Opens a multipart upload for a file longer than one part.
    ///
    /// **Nothing is visible at `reference` until [`PartUpload::complete`].** So a
    /// refusal that arrives with the last bytes of a body still leaves no object,
    /// and the row-after-object order [`Self::put`] describes holds here too.
    pub async fn begin_upload(&self, reference: &str) -> Result<PartUpload, AppError> {
        match self {
            Self::S3 { store, bucket } => {
                let upload = store
                    .put_multipart(&ObjectPath::from(reference))
                    .await
                    .map_err(|error| {
                        tracing::error!(
                            %error,
                            %bucket,
                            %reference,
                            "an attachment's upload could not be opened"
                        );

                        write_refused(&error)
                    })?;

                Ok(PartUpload {
                    upload: Some(upload),
                    reference: reference.to_owned(),
                })
            }
            Self::Unavailable { reason } => Err(no_store(reason)),
        }
    }
}

fn write_refused(error: &object_store::Error) -> AppError {
    AppError::Internal {
        source: anyhow::anyhow!("object storage refused the write: {error}"),
    }
}

fn no_store(reason: &str) -> AppError {
    AppError::Internal {
        source: anyhow::anyhow!(
            "this deployment has no object storage configured, so a file cannot be \
             attached: {reason}. Set KELIR_STORAGE_ENDPOINT, KELIR_STORAGE_BUCKET \
             and the two credentials"
        ),
    }
}

/// A multipart upload under way, which aborts itself unless it is completed.
///
/// # Why the abort is in `Drop`
///
/// **A client that disconnects can drop the handler's future at any `await`, and
/// then no error path runs.** S3 keeps the parts of an upload nobody aborted, out
/// of sight and billed, so every way out of a request has to end the upload.
/// Each refusal is a way out as well, so there is one path for all of them
/// rather than a call beside each `?`: dropping this.
///
/// `Drop` cannot wait, so the abort is spawned, and a response can leave before
/// it lands. A process killed outright runs nothing; what that leaves is hidden
/// parts, never an object, and the store's stale-upload expiry is what clears
/// them.
///
/// **Sent one part at a time**, by the handler's own task. A part is what holds
/// memory, so the next is not read until the store has taken this one, and since
/// nothing is spawned, a dropped request leaves no part still being sent.
pub struct PartUpload {
    /// `None` once [`Self::complete`] has succeeded, which is the end of this.
    upload: Option<Box<dyn MultipartUpload>>,
    reference: String,
}

impl PartUpload {
    /// Sends one part and waits for the store to take it.
    ///
    /// Every part but the last must be [`PART_SIZE`].
    pub async fn put_part(&mut self, part: Bytes) -> Result<(), AppError> {
        if let Some(upload) = self.upload.as_mut() {
            upload
                .put_part(PutPayload::from_bytes(part))
                .await
                .map_err(|error| {
                    tracing::error!(
                        %error,
                        reference = %self.reference,
                        "a part of an attachment could not be stored"
                    );

                    write_refused(&error)
                })?;
        }

        Ok(())
    }

    /// Makes the object exist, and stands the guard down.
    ///
    /// A `complete` that fails keeps the upload, so it is aborted like any other.
    pub async fn complete(mut self) -> Result<(), AppError> {
        if let Some(upload) = self.upload.as_mut() {
            upload.complete().await.map_err(|error| {
                tracing::error!(
                    %error,
                    reference = %self.reference,
                    "an attachment's upload could not be completed"
                );

                write_refused(&error)
            })?;
        }

        self.upload = None;

        Ok(())
    }
}

impl Drop for PartUpload {
    fn drop(&mut self) {
        let Some(mut upload) = self.upload.take() else {
            return;
        };

        let reference = std::mem::take(&mut self.reference);

        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                %reference,
                "an upload ended with no runtime to abort it; its parts stay until the store \
                 expires them"
            );

            return;
        };

        runtime.spawn(async move {
            if let Err(error) = upload.abort().await {
                tracing::warn!(
                    %error,
                    %reference,
                    "an upload could not be aborted; its parts stay until the store expires them"
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    //! **The request is read off the wire**, from a socket this test owns,
    //! because what NFR-SEC-009 needs is the header MinIO receives. A store
    //! with a key encrypts either way when the bucket default is set, so a
    //! test against MinIO cannot tell a backend that asks from one that
    //! relies on the default (ADR-0049 §3, *fails closed*).

    use super::*;
    use object_store::ClientConfigKey;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    /// Answers one request with `reply` and hands back the request's head,
    /// lower-cased so the header can be matched whatever case it was sent in.
    async fn one_request(reply: String) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let endpoint = format!("http://{}", listener.local_addr().expect("an address"));

        let served = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("a request");
            let mut received = Vec::new();
            let mut chunk = [0_u8; 4096];

            let head_end = loop {
                let read = socket.read(&mut chunk).await.expect("the request");
                assert!(read > 0, "the client hung up before its head ended");
                received.extend_from_slice(&chunk[..read]);

                if let Some(at) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    break at + 4;
                }
            };

            let head = String::from_utf8_lossy(&received[..head_end]).to_lowercase();
            let body_length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse().expect("a length"));

            while received.len() < head_end + body_length {
                let read = socket.read(&mut chunk).await.expect("the body");
                assert!(read > 0, "the client hung up mid-body");
                received.extend_from_slice(&chunk[..read]);
            }

            socket.write_all(reply.as_bytes()).await.expect("the reply");
            socket.shutdown().await.ok();

            head
        });

        (endpoint, served)
    }

    /// **Read off the builder, because the failure takes minutes to show.** The
    /// measurement is on [`STORE_REQUEST_TIMEOUT`]; this holds that it is the
    /// store's setting and that the 30-second default is not.
    #[test]
    fn a_request_to_the_store_may_last_as_long_as_a_slow_download() {
        let timeout = ObjectStorage::builder(&AppConfig::test_default())
            .get_config_value(&AmazonS3ConfigKey::Client(ClientConfigKey::Timeout));

        assert_eq!(timeout.as_deref(), Some("30m"));
    }

    fn asks_for_sse_s3(head: &str) -> bool {
        head.lines()
            .any(|line| line.trim_end() == "x-amz-server-side-encryption: aes256")
    }

    /// The one writer the product has (#778, AC3). Seen red, 2026-10-28.
    #[tokio::test]
    async fn every_object_written_asks_the_store_to_encrypt_it() {
        let (endpoint, served) = one_request(
            "HTTP/1.1 200 OK\r\nETag: \"row2\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_owned(),
        )
        .await;

        let mut config = AppConfig::test_default();
        config.storage_endpoint = endpoint;
        ObjectStorage::from_config(&config)
            .put(
                "tenants/t/documents/d/attachments/a/q.pdf",
                Bytes::from_static(b"%PDF-1.7"),
            )
            .await
            .expect("the scripted store accepts the write");

        let head = served.await.expect("the request was served");
        assert!(head.starts_with("put "), "{head}");
        assert!(
            asks_for_sse_s3(&head),
            "the write did not ask for SSE-S3:\n{head}"
        );
    }

    /// **A multipart upload asks when it is created**, which is where S3 takes
    /// SSE-S3 for one; its parts carry the header only for SSE-C. It inherits
    /// the setting because that is on the store, not on [`ObjectStorage::put`].
    /// Seen red, 2026-10-28.
    #[tokio::test]
    async fn a_multipart_upload_asks_the_store_to_encrypt_it_too() {
        let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
            <InitiateMultipartUploadResult><Bucket>kelir</Bucket><Key>k</Key>\
            <UploadId>row2</UploadId></InitiateMultipartUploadResult>";
        let (endpoint, served) = one_request(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        ))
        .await;

        let mut config = AppConfig::test_default();
        config.storage_endpoint = endpoint;

        // Stood down rather than dropped: this store answers one request, and
        // the guard's abort would be a second.
        let upload = ObjectStorage::from_config(&config)
            .begin_upload("tenants/t/large.pdf")
            .await
            .expect("the scripted store starts the upload");
        std::mem::forget(upload);

        let head = served.await.expect("the request was served");
        assert!(
            head.starts_with("post ") && head.contains("?uploads"),
            "{head}"
        );
        assert!(
            asks_for_sse_s3(&head),
            "the upload did not ask for SSE-S3:\n{head}"
        );
    }
}
