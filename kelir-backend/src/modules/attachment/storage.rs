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
//! to put and get objects in a bucket somebody else provisioned; a process that
//! can create buckets can create the one an attacker names. `deploy/docker`
//! creates it, and CI's backend job creates it before the tests run.
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use std::sync::Arc;

use axum::body::Bytes;
use object_store::aws::{AmazonS3Builder, AmazonS3ConfigKey};
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, PutPayload};

use crate::config::AppConfig;
use crate::error::AppError;

/// `x-amz-server-side-encryption`'s value for SSE-S3: the store encrypts under
/// its own key (`MINIO_KMS_SECRET_KEY`).
const SSE_S3: &str = "AES256";

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
    /// Reads one object back.
    ///
    /// **A missing object is an internal error, not a 404.** The caller reached
    /// here by naming a row that exists and that they may read; if the bytes it
    /// points at are gone, the product is inconsistent and saying *not found*
    /// would report that as the caller's mistake. [`super::service::upload`]
    /// writes the object before the row precisely so this cannot happen, so
    /// reaching it means something outside this service removed an object.
    pub async fn get(&self, reference: &str) -> Result<axum::body::Bytes, AppError> {
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

                object.bytes().await.map_err(|error| {
                    tracing::error!(%error, %bucket, %reference, "an attachment's bytes stopped mid-read");

                    AppError::Internal {
                        source: anyhow::anyhow!("object storage stopped mid-read: {error}"),
                    }
                })
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
                AmazonS3Builder::new()
                    .with_endpoint(&config.storage_endpoint)
                    .with_bucket_name(&config.storage_bucket)
                    .with_access_key_id(&config.storage_access_key)
                    .with_secret_access_key(&config.storage_secret_key)
                    .with_region(&config.storage_region)
                    .with_allow_http(true)
                    .with_virtual_hosted_style_request(false)
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

    /// Writes one object, and is the only thing in this codebase that does.
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
            Self::Unavailable { reason } => Err(AppError::Internal {
                source: anyhow::anyhow!(
                    "this deployment has no object storage configured, so a file cannot be \
                     attached: {reason}. Set KELIR_STORAGE_ENDPOINT, KELIR_STORAGE_BUCKET \
                     and the two credentials"
                ),
            }),
        }
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

    fn storage_at(endpoint: String) -> Arc<dyn ObjectStore> {
        let mut config = AppConfig::test_default();
        config.storage_endpoint = endpoint;

        match ObjectStorage::from_config(&config) {
            ObjectStorage::S3 { store, .. } => store,
            ObjectStorage::Unavailable { reason } => panic!("no store was built: {reason}"),
        }
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
    /// SSE-S3 for one; its parts carry the header only for SSE-C. Nothing
    /// writes multipart today, and a writer that does inherits the setting
    /// because it is on the store, not on [`ObjectStorage::put`]. Seen red,
    /// 2026-10-28.
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

        storage_at(endpoint)
            .put_multipart(&ObjectPath::from("tenants/t/large.pdf"))
            .await
            .expect("the scripted store starts the upload");

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
