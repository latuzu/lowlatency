//! gRPC service implementation.

use crate::metrics::Metrics;
use crate::proto::{
    kv_service_server::KvService, BatchGetRequest, BatchGetResponse, GetRequest, GetResponse,
    HealthRequest, HealthResponse,
};
use arc_swap::ArcSwap;
use kv_store::{KvStore, KEY_SIZE};
use std::sync::Arc;
use std::time::Instant;
use tonic::{Request, Response, Status};
use tracing::warn;

/// gRPC service implementation for KV lookups.
pub struct KvServiceImpl {
    /// The KV store, wrapped in ArcSwap for hot reload support
    store: Arc<ArcSwap<KvStore>>,
    /// Prometheus metrics
    metrics: Arc<Metrics>,
}

impl KvServiceImpl {
    /// Creates a new service instance.
    pub fn new(store: KvStore, metrics: Arc<Metrics>) -> Self {
        Self {
            store: Arc::new(ArcSwap::new(Arc::new(store))),
            metrics,
        }
    }

    /// Hot-reloads the store with a new snapshot.
    ///
    /// This is atomic and lock-free. Existing requests complete with the old
    /// snapshot while new requests use the new one.
    pub fn reload(&self, new_store: KvStore) {
        self.store.store(Arc::new(new_store));
    }
}

#[tonic::async_trait]
impl KvService for KvServiceImpl {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        let start = Instant::now();

        let req = request.into_inner();
        let key = &req.key;

        // Validate key size
        if key.len() != KEY_SIZE {
            self.metrics.increment_errors();
            return Err(Status::invalid_argument(format!(
                "Key must be exactly {} bytes, got {}",
                KEY_SIZE,
                key.len()
            )));
        }

        // Convert to fixed-size array
        let key: &[u8; KEY_SIZE] = key.as_slice().try_into().map_err(|_| {
            self.metrics.increment_errors();
            Status::internal("Key conversion failed")
        })?;

        // Load current store (lock-free)
        let store = self.store.load();

        // Perform lookup
        let response = match store.get(key) {
            Some(value) => {
                self.metrics.increment_hits();
                GetResponse {
                    found: true,
                    value: value.to_vec(),
                }
            }
            None => {
                self.metrics.increment_misses();
                GetResponse {
                    found: false,
                    value: vec![],
                }
            }
        };

        // Record latency
        self.metrics.record_latency(start.elapsed());
        self.metrics.increment_requests();

        Ok(Response::new(response))
    }

    async fn batch_get(
        &self,
        request: Request<BatchGetRequest>,
    ) -> Result<Response<BatchGetResponse>, Status> {
        let start = Instant::now();

        let req = request.into_inner();
        let keys = &req.keys;

        // Load current store once for all lookups
        let store = self.store.load();

        let mut results = Vec::with_capacity(keys.len());

        for key in keys {
            if key.len() != KEY_SIZE {
                self.metrics.increment_errors();
                return Err(Status::invalid_argument(format!(
                    "All keys must be exactly {} bytes",
                    KEY_SIZE
                )));
            }

            let key: &[u8; KEY_SIZE] = key.as_slice().try_into().map_err(|_| {
                self.metrics.increment_errors();
                Status::internal("Key conversion failed")
            })?;

            let result = match store.get(key) {
                Some(value) => {
                    self.metrics.increment_hits();
                    GetResponse {
                        found: true,
                        value: value.to_vec(),
                    }
                }
                None => {
                    self.metrics.increment_misses();
                    GetResponse {
                        found: false,
                        value: vec![],
                    }
                }
            };

            results.push(result);
        }

        // Record latency (for entire batch)
        self.metrics.record_latency(start.elapsed());
        self.metrics.increment_requests();

        Ok(Response::new(BatchGetResponse { results }))
    }

    async fn health(&self, _request: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        let store = self.store.load();
        let info = store.version_info();

        Ok(Response::new(HealthResponse {
            healthy: true,
            version: format!("v{}", info.build_timestamp),
            record_count: info.record_count,
            build_timestamp: info.build_timestamp,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kv_store::KvStore;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_store(record_count: usize) -> (NamedTempFile, KvStore) {
        // Use snapshot-builder logic to create test store
        use kv_store::{Header, Mphf, RECORD_SIZE};

        let mut file = NamedTempFile::new().unwrap();

        let keys: Vec<[u8; 64]> = (0..record_count as u64)
            .map(|i| {
                let mut key = [0u8; 64];
                key[0..8].copy_from_slice(&i.to_le_bytes());
                key
            })
            .collect();

        let values: Vec<[u8; 256]> = (0..record_count as u64)
            .map(|i| {
                let mut value = [0u8; 256];
                value[0..8].copy_from_slice(&(i * 100).to_le_bytes());
                value
            })
            .collect();

        let mphf = Mphf::build(keys.iter()).unwrap();
        let mphf_bytes = mphf.serialize().unwrap();

        let header = Header::new(record_count as u64, mphf_bytes.len() as u64, 0);
        let data_offset = header.data_offset as usize;
        let header_size = kv_store::header::HEADER_SIZE;

        let file_size = data_offset + record_count * RECORD_SIZE;
        file.as_file().set_len(file_size as u64).unwrap();

        file.write_all(&header.to_bytes()).unwrap();
        file.write_all(&mphf_bytes).unwrap();

        let current_pos = header_size + mphf_bytes.len();
        if current_pos < data_offset {
            file.write_all(&vec![0u8; data_offset - current_pos]).unwrap();
        }

        let mut records = vec![[0u8; RECORD_SIZE]; record_count];
        for (key, value) in keys.iter().zip(values.iter()) {
            let slot = mphf.get(key).unwrap();
            records[slot][0..64].copy_from_slice(key);
            records[slot][64..RECORD_SIZE].copy_from_slice(value);
        }
        for record in records {
            file.write_all(&record).unwrap();
        }

        file.flush().unwrap();

        let store = KvStore::open(file.path()).unwrap();
        (file, store)
    }

    #[tokio::test]
    async fn test_get_found() {
        let (_file, store) = create_test_store(100);
        let metrics = Arc::new(Metrics::new());
        let service = KvServiceImpl::new(store, metrics);

        let mut key = vec![0u8; 64];
        key[0..8].copy_from_slice(&42u64.to_le_bytes());

        let request = Request::new(GetRequest { key });
        let response = service.get(request).await.unwrap().into_inner();

        assert!(response.found);
        let value = u64::from_le_bytes(response.value[0..8].try_into().unwrap());
        assert_eq!(value, 4200);
    }

    #[tokio::test]
    async fn test_get_not_found() {
        let (_file, store) = create_test_store(100);
        let metrics = Arc::new(Metrics::new());
        let service = KvServiceImpl::new(store, metrics);

        let mut key = vec![0u8; 64];
        key[0..8].copy_from_slice(&9999u64.to_le_bytes());

        let request = Request::new(GetRequest { key });
        let response = service.get(request).await.unwrap().into_inner();

        assert!(!response.found);
        assert!(response.value.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_key_size() {
        let (_file, store) = create_test_store(10);
        let metrics = Arc::new(Metrics::new());
        let service = KvServiceImpl::new(store, metrics);

        let request = Request::new(GetRequest {
            key: vec![0u8; 32], // Wrong size
        });

        let result = service.get(request).await;
        assert!(result.is_err());
    }
}
