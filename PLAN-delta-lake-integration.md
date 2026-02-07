# Plan: Add Delta Lake Output to certstream-server-rust

## Goal

Modify [burakozcn01/certstream-server-rust](https://github.com/burakozcn01/certstream-server-rust) so that it can write certificates directly to a Delta Lake table, eliminating the need for a separate websocket consumer.

## Context

The server already polls 60+ CT logs, parses X.509 certs, and broadcasts `CertificateMessage` structs through a `tokio::sync::broadcast` channel. WebSocket and SSE handlers subscribe to this channel. We add a Delta Lake writer that does the same.

---

## Architecture

```
CT Logs ──► [watcher tasks] ──► broadcast_cert() ──► broadcast channel
                                                         │
                                         ┌───────────────┼───────────────┐
                                         ▼               ▼               ▼
                                    WebSocket           SSE        DeltaLakeWriter
                                    (existing)       (existing)       (new)
```

The Delta Lake writer is a long-lived tokio task that subscribes to the existing broadcast channel, buffers messages into Arrow RecordBatches, and periodically flushes them to a Delta Lake table.

---

## Step-by-step Implementation

### Step 1: Add dependencies to `Cargo.toml`

Add these under `[dependencies]`:

```toml
deltalake = { version = "0.22", features = ["datafusion"] }
arrow = { version = "53", features = ["prettyprint"] }
object_store = "0.11"
```

Pin versions to whatever is current. The `deltalake` crate re-exports `arrow` and `object_store`, so you may be able to use `deltalake::arrow` instead of a separate `arrow` dep. Check compatibility.

Note: The version in `1mentat/certstream-rust` (0.6.0) is very old. Use latest.

### Step 2: Add configuration

**`src/config.rs`** — Add a new config section:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaLakeConfig {
    /// Enable/disable Delta Lake writing
    pub enabled: bool,              // default: false
    /// Table URI (local path or S3/GCS/Azure URL)
    pub table_uri: String,          // default: "./certstream.delta"
    /// Flush interval in seconds
    pub flush_interval_secs: u64,   // default: 60
    /// Max records to buffer before forcing a flush
    pub batch_size: usize,          // default: 10_000
    /// Storage mode: "raw" (single JSON column) or "columnar" (structured fields)
    pub storage_mode: String,       // default: "raw"
}
```

Add `pub delta_lake: DeltaLakeConfig` to the `Config` struct.

Add env var overrides following the existing pattern:
- `CERTSTREAM_DELTA_LAKE_ENABLED`
- `CERTSTREAM_DELTA_LAKE_TABLE_URI`
- `CERTSTREAM_DELTA_LAKE_FLUSH_INTERVAL_SECS`
- `CERTSTREAM_DELTA_LAKE_BATCH_SIZE`
- `CERTSTREAM_DELTA_LAKE_STORAGE_MODE`

### Step 3: Create `src/delta_lake.rs` — the writer module

This is the core new file. It contains:

#### 3a. Schema definitions

Two schemas depending on `storage_mode`:

**Raw mode** — single JSON string column (simple, preserves everything):
```rust
fn raw_schema() -> ArrowSchema {
    ArrowSchema::new(vec![
        Field::new("raw", DataType::Utf8, false),
    ])
}
```

**Columnar mode** — structured fields for query performance:
```rust
fn columnar_schema() -> ArrowSchema {
    ArrowSchema::new(vec![
        Field::new("fingerprint", DataType::Utf8, false),
        Field::new("sha256", DataType::Utf8, false),
        Field::new("serial_number", DataType::Utf8, false),
        Field::new("subject_cn", DataType::Utf8, true),
        Field::new("issuer_cn", DataType::Utf8, true),
        Field::new("not_before", DataType::Int64, false),
        Field::new("not_after", DataType::Int64, false),
        Field::new("is_ca", DataType::Boolean, false),
        Field::new("all_domains", DataType::Utf8, false),      // JSON array as string
        Field::new("signature_algorithm", DataType::Utf8, false),
        Field::new("seen", DataType::Float64, false),
        Field::new("source_name", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cert_index", DataType::UInt64, false),
        Field::new("update_type", DataType::Utf8, false),
    ])
}
```

Start with raw mode. Add columnar later if query performance matters.

#### 3b. `DeltaLakeWriter` struct

```rust
pub struct DeltaLakeWriter {
    config: DeltaLakeConfig,
    table: DeltaTable,
    writer: RecordBatchWriter,
    buffer: Vec<Arc<PreSerializedMessage>>,
    last_flush: Instant,
}
```

#### 3c. Table initialization

Open-or-create pattern:

```rust
impl DeltaLakeWriter {
    pub async fn new(config: DeltaLakeConfig) -> Result<Self, DeltaTableError> {
        let table = match deltalake::open_table(&config.table_uri).await {
            Ok(table) => table,
            Err(DeltaTableError::NotATable(_)) => {
                Self::create_table(&config).await?
            }
            Err(e) => return Err(e),
        };
        let writer = RecordBatchWriter::for_table(&table)?;
        Ok(Self {
            config,
            table,
            writer,
            buffer: Vec::with_capacity(10_000),
            last_flush: Instant::now(),
        })
    }
}
```

#### 3d. Batch conversion

Convert buffered `PreSerializedMessage` objects to Arrow `RecordBatch`:

```rust
fn build_batch(&self) -> RecordBatch {
    // For raw mode: serialize each message's `full` field as a string
    let json_strings: Vec<String> = self.buffer.iter()
        .map(|msg| String::from_utf8_lossy(&msg.full).into_owned())
        .collect();
    let refs: Vec<&str> = json_strings.iter().map(|s| s.as_str()).collect();
    let array: ArrayRef = Arc::new(StringArray::from(refs));
    RecordBatch::try_new(Arc::new(raw_schema()), vec![array]).unwrap()
}
```

For columnar mode, you'd instead deserialize each `msg.full` back to `CertificateMessage` and extract fields into separate arrays. This is more work but produces a much more queryable table.

#### 3e. Flush logic

```rust
async fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    if self.buffer.is_empty() {
        return Ok(());
    }
    let batch = self.build_batch();
    self.writer.write(batch).await?;
    self.writer.flush_and_commit(&mut self.table).await?;
    tracing::info!(
        rows = self.buffer.len(),
        "Flushed batch to Delta Lake"
    );
    self.buffer.clear();
    self.last_flush = Instant::now();
    Ok(())
}
```

#### 3f. The run loop (long-lived tokio task)

```rust
pub async fn run(
    mut self,
    mut rx: broadcast::Receiver<Arc<PreSerializedMessage>>,
    shutdown: CancellationToken,
) {
    let flush_interval = Duration::from_secs(self.config.flush_interval_secs);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                // Final flush on shutdown
                if let Err(e) = self.flush().await {
                    tracing::error!("Final Delta Lake flush failed: {e}");
                }
                break;
            }
            msg = rx.recv() => {
                match msg {
                    Ok(msg) => {
                        self.buffer.push(msg);
                        if self.buffer.len() >= self.config.batch_size {
                            if let Err(e) = self.flush().await {
                                tracing::error!("Delta Lake flush failed: {e}");
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Delta Lake writer lagged, dropped {n} messages");
                        // Continue — some loss is acceptable
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = tokio::time::sleep(flush_interval) => {
                if self.last_flush.elapsed() >= flush_interval {
                    if let Err(e) = self.flush().await {
                        tracing::error!("Delta Lake periodic flush failed: {e}");
                    }
                }
            }
        }
    }
}
```

### Step 4: Wire it into `src/main.rs`

In the server startup, after the broadcast channel is created and before the axum router is built:

```rust
// Existing code creates the broadcast channel:
// let (tx, _) = broadcast::channel::<Arc<PreSerializedMessage>>(config.buffer_size);

if config.delta_lake.enabled {
    let dl_rx = tx.subscribe();
    let dl_config = config.delta_lake.clone();
    let dl_shutdown = shutdown_token.clone();
    tokio::spawn(async move {
        match DeltaLakeWriter::new(dl_config).await {
            Ok(writer) => writer.run(dl_rx, dl_shutdown).await,
            Err(e) => tracing::error!("Failed to initialize Delta Lake writer: {e}"),
        }
    });
}
```

Add `mod delta_lake;` to the module declarations.

### Step 5: Handle broadcast channel sizing

The default `buffer_size` is 1000. If the Delta Lake writer is slow (e.g. writing to S3), it could lag behind and miss messages. Consider:

- Increasing `buffer_size` to 10,000+ when Delta Lake is enabled
- Or: add an intermediate `mpsc` channel between broadcast and writer with larger capacity
- The `Lagged` error in the run loop handles this gracefully but logs the data loss

### Step 6: Optional — disable websocket/SSE when only writing

If the server is only used for Delta Lake ingestion (no streaming clients), allow disabling WebSocket/SSE to save resources:

```yaml
protocols:
  websocket: false
  sse: false
delta_lake:
  enabled: true
```

The existing `ProtocolConfig` already supports toggling websocket and SSE. Just make sure the broadcast channel and watchers still run even when protocols are disabled.

### Step 7: Update `config.example.yaml`

Add the new section:

```yaml
delta_lake:
  enabled: false
  table_uri: "./certstream.delta"
  flush_interval_secs: 60
  batch_size: 10000
  storage_mode: "raw"    # "raw" or "columnar"
```

### Step 8: Add metrics

In the flush method, emit Prometheus metrics using the existing `metrics` crate:

```rust
metrics::counter!("certstream_delta_lake_rows_written").increment(self.buffer.len() as u64);
metrics::counter!("certstream_delta_lake_flushes").increment(1);
metrics::histogram!("certstream_delta_lake_flush_duration_seconds").record(elapsed);
```

### Step 9: Tests

- **Unit test**: Schema creation produces valid Arrow schema with expected fields
- **Unit test**: Batch conversion from mock `PreSerializedMessage` produces correct RecordBatch
- **Integration test**: Create a temp Delta Lake table, write records, read them back, verify contents
- **Integration test**: Writer handles `Lagged` errors without crashing

---

## Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `Cargo.toml` | Modify | Add `deltalake`, `arrow`, `object_store` deps |
| `src/delta_lake.rs` | **Create** | Writer module (~200 lines) |
| `src/main.rs` | Modify | Add `mod delta_lake`, spawn writer task |
| `src/config.rs` | Modify | Add `DeltaLakeConfig` struct and defaults |
| `config.example.yaml` | Modify | Add `delta_lake` section |
| `tests/delta_lake.rs` | **Create** | Integration tests |

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| `deltalake` crate version conflicts with existing deps | Check Arrow/Tokio version alignment before starting. The server uses tokio 1.48, arrow not currently in deps. |
| Writer too slow, lags behind broadcast | Buffer in mpsc channel; accept some loss; log `Lagged` count as metric |
| S3/GCS writes add latency to flush | Flush is async and doesn't block the broadcast channel or other consumers |
| Table gets too many small Parquet files | Increase `batch_size` / `flush_interval_secs`; run `OPTIMIZE` compaction periodically (external process or future feature) |
| Schema evolution | Raw mode avoids this entirely. Columnar mode would need migration strategy. Start with raw. |

---

## Future enhancements (out of scope)

- Partitioning by date (`seen` timestamp) for faster time-range queries
- Columnar mode with full field extraction
- `OPTIMIZE` / `VACUUM` commands on a schedule
- Filtering (only write certs matching certain domains/CAs)
- Dual output: write to Delta Lake AND serve websocket simultaneously (already works by default since both subscribe to the same broadcast channel)
