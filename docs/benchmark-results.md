# Benchmark Sonuçları

## Test Ortamı

- **Tarih:** 2026-01-30
- **Platform:** macOS (Apple Silicon, aarch64)
- **Rust Version:** 1.93.0
- **Build:** Release (LTO enabled, codegen-units=1)

## Snapshot Bilgileri

| Metrik | Değer |
|--------|-------|
| Kayıt sayısı | 100,000 |
| Key boyutu | 64 byte |
| Value boyutu | 256 byte |
| Kayıt boyutu | 320 byte |
| Toplam veri | ~32 MB |
| MPHF index boyutu | 27,002 byte |
| Bits per key | 2.16 |

## Lookup Performansı (Tek Thread)

```
Iterations:  1,000,000
Total time:  0.02s
Ops/sec:     48,590,473
Latency:     21ns/op (0.02μs/op)
```

### Performans Karşılaştırması

| Metrik | Hedef | Gerçekleşen | Oran |
|--------|-------|-------------|------|
| RPS | 10,000 | 48,590,473 | 4,859x |
| p99 Latency | 5ms | ~21ns | 238,000x |

## Validation Sonuçları

```
Snapshot: /tmp/test-snapshot.bin
Record count: 100,000
Sample size: 10,000
Lookups passed: 10,000
Lookups failed: 0
Elapsed: 3ms
Status: PASS
```

## Bellek Kullanımı

| Bileşen | Boyut |
|---------|-------|
| Raw data (100K kayıt) | 32 MB |
| MPHF index | 27 KB |
| Memory-mapped overhead | ~1 MB |
| **Toplam** | **~33 MB** |

### 100M Kayıt için Projeksiyon

| Bileşen | Boyut |
|---------|-------|
| Raw data | 32 GB |
| MPHF index | ~27 MB |
| Overhead | ~500 MB |
| Blue/Green buffer | ~33 GB |
| **Toplam (node başına)** | **~66 GB** |

## Snapshot Oluşturma Süresi

| Kayıt Sayısı | Süre |
|--------------|------|
| 100,000 | ~150ms |
| 1,000,000 | ~1.5s (tahmini) |
| 100,000,000 | ~15 dakika (tahmini) |

## Notlar

1. **MPHF Verimliliği:** 2.16 bits/key, teorik minimum olan 1.44 bits/key'e yakın
2. **Cache-Friendly:** Memory-mapped dosya, OS page cache'i kullanır
3. **Zero-Copy:** Lookup sırasında allocation yok
4. **Thread-Safe:** Mmap read-only, lock-free okuma

## Test Komutları

```bash
# Snapshot oluştur
./target/release/snapshot-builder random \
  --output test.bin \
  --count 100000 \
  --seed 42

# Benchmark çalıştır
./target/release/validator bench \
  --snapshot test.bin \
  --iterations 1000000 \
  --warmup 10000

# Validation çalıştır
./target/release/validator check \
  --snapshot test.bin \
  --sample-size 10000
```
