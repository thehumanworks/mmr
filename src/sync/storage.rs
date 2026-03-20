use std::collections::BTreeMap;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use s3::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

use crate::sync::config::SyncConfig;

#[derive(Debug, Clone)]
pub struct RemoteObject {
    pub key: String,
    pub size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub size: u64,
    pub etag: Option<String>,
}

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn put_object(&self, key: &str, body: &[u8]) -> Result<String>;
    async fn get_object(&self, key: &str) -> Result<Vec<u8>>;
    async fn list_objects(&self, prefix: &str) -> Result<Vec<RemoteObject>>;
    async fn head_object(&self, key: &str) -> Result<Option<ObjectMeta>>;
}

// --- R2 (S3-compatible) implementation ---

pub struct R2Storage {
    bucket: Box<Bucket>,
}

impl R2Storage {
    pub fn new(config: &SyncConfig) -> Result<Self> {
        let region = Region::Custom {
            region: config.storage.region.clone(),
            endpoint: config.storage.endpoint.clone(),
        };

        let credentials = Credentials::new(
            Some(&config.storage.access_key_id),
            Some(&config.storage.secret_access_key),
            None,
            None,
            None,
        )?;

        let bucket = Bucket::new(&config.storage.bucket, region, credentials)?.with_path_style();

        Ok(Self { bucket })
    }
}

#[async_trait]
impl StorageBackend for R2Storage {
    async fn put_object(&self, key: &str, body: &[u8]) -> Result<String> {
        let response = self.bucket.put_object(key, body).await?;
        let etag = response.headers().get("etag").cloned().unwrap_or_default();
        Ok(etag)
    }

    async fn get_object(&self, key: &str) -> Result<Vec<u8>> {
        let response = self.bucket.get_object(key).await?;
        Ok(response.to_vec())
    }

    async fn list_objects(&self, prefix: &str) -> Result<Vec<RemoteObject>> {
        let mut results = Vec::new();
        let pages = self.bucket.list(prefix.to_string(), None).await?;

        for page in &pages {
            for item in &page.contents {
                results.push(RemoteObject {
                    key: item.key.clone(),
                    size: item.size,
                    etag: item.e_tag.clone(),
                    last_modified: Some(item.last_modified.clone()),
                });
            }
        }

        Ok(results)
    }

    async fn head_object(&self, key: &str) -> Result<Option<ObjectMeta>> {
        match self.bucket.head_object(key).await {
            Ok((head, _code)) => Ok(Some(ObjectMeta {
                size: head.content_length.unwrap_or(0) as u64,
                etag: head.e_tag,
            })),
            Err(_) => Ok(None),
        }
    }
}

// --- In-memory implementation for tests ---

pub struct MemoryStorage {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            objects: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageBackend for MemoryStorage {
    async fn put_object(&self, key: &str, body: &[u8]) -> Result<String> {
        let mut store = self.objects.lock().unwrap();
        store.insert(key.to_string(), body.to_vec());
        Ok(format!("\"mem-etag-{}\"", key.len()))
    }

    async fn get_object(&self, key: &str) -> Result<Vec<u8>> {
        let store = self.objects.lock().unwrap();
        store
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("object not found: {}", key))
    }

    async fn list_objects(&self, prefix: &str) -> Result<Vec<RemoteObject>> {
        let store = self.objects.lock().unwrap();
        Ok(store
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| RemoteObject {
                key: k.clone(),
                size: v.len() as u64,
                etag: Some(format!("\"mem-etag-{}\"", v.len())),
                last_modified: None,
            })
            .collect())
    }

    async fn head_object(&self, key: &str) -> Result<Option<ObjectMeta>> {
        let store = self.objects.lock().unwrap();
        Ok(store.get(key).map(|v| ObjectMeta {
            size: v.len() as u64,
            etag: Some(format!("\"mem-etag-{}\"", v.len())),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_storage_put_get() {
        let store = MemoryStorage::new();
        store.put_object("test/file.txt", b"hello").await.unwrap();
        let data = store.get_object("test/file.txt").await.unwrap();
        assert_eq!(data, b"hello");
    }

    #[tokio::test]
    async fn memory_storage_list() {
        let store = MemoryStorage::new();
        store.put_object("a/1.txt", b"one").await.unwrap();
        store.put_object("a/2.txt", b"two").await.unwrap();
        store.put_object("b/3.txt", b"three").await.unwrap();

        let listed = store.list_objects("a/").await.unwrap();
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn memory_storage_head_missing() {
        let store = MemoryStorage::new();
        let meta = store.head_object("missing").await.unwrap();
        assert!(meta.is_none());
    }

    #[tokio::test]
    async fn memory_storage_get_missing_errors() {
        let store = MemoryStorage::new();
        assert!(store.get_object("missing").await.is_err());
    }
}
