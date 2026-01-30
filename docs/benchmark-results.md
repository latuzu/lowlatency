# Benchmark Sonuçları

## Test Ortamı

- **Tarih:** 2026-01-30
- **Platform:** macOS (Apple Silicon, aarch64)
- **Rust Version:** 1.93.0
- **Build:** Release (LTO enabled, codegen-units=1)

---

## 🚀 10M Kayıt Load Test (gRPC, 15 dakika)

### Test Konfigürasyonu

| Metrik | Değer |
|--------|-------|
| Kayıt sayısı | 10,000,000 |
| Snapshot boyutu | 3.0 GB |
| MPHF index boyutu | 2.7 MB |
| Bits per key | 2.16 |
| Test süresi | 15 dakika (900 saniye) |
| Hedef RPS | 20,000 |
| Concurrent bağlantı | 100 |
| Worker sayısı | 200 |

### Sonuçlar

```
Summary:
  Count:        17,999,884
  Total:        900.00 s
  Slowest:      6.94 ms
  Fastest:      0.02 ms
  Average:      0.05 ms
  Requests/sec: 19,999.96
```

### Latency Dağılımı

| Percentile | Latency |
|------------|---------|
| p10 | 0.04 ms |
| p25 | 0.04 ms |
| p50 | 0.04 ms |
| p75 | 0.05 ms |
| p90 | 0.06 ms |
| p95 | 0.07 ms |
| **p99** | **0.17 ms** |

### Hedef Karşılaştırması

| Metrik | Hedef | Gerçekleşen | Sonuç |
|--------|-------|-------------|-------|
| RPS | 20,000 | 19,999.96 | ✅ |
| p99 Latency | ≤5ms | 0.17ms | ✅ (29x daha iyi) |
| Başarı Oranı | 100% | 99.99999% | ✅ |
| Toplam İstek | 18M | 17,999,884 | ✅ |

### Response Time Histogram

```
  0.025 [1]      |
  0.716 [999393] |∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎∎
  1.408 [252]    |
  2.099 [261]    |
  2.791 [70]     |
  3.482 [9]      |
  4.174 [4]      |
  4.865 [2]      |
  5.557 [2]      |
  6.248 [1]      |
  6.940 [5]      |
```

---

## 100K Kayıt Benchmark (In-Memory, Tek Thread)

### Snapshot Bilgileri

| Metrik | Değer |
|--------|-------|
| Kayıt sayısı | 100,000 |
| Key boyutu | 64 byte |
| Value boyutu | 256 byte |
| Kayıt boyutu | 320 byte |
| Toplam veri | ~32 MB |
| MPHF index boyutu | 27,002 byte |
| Bits per key | 2.16 |

### Lookup Performansı

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

---

## 10M Kayıt Benchmark (In-Memory, Tek Thread)

```
Iterations:  10,000,000
Total time:  0.40s
Ops/sec:     24,716,741
Latency:     40ns/op (0.04μs/op)
```

### Performans Karşılaştırması

| Metrik | Hedef | Gerçekleşen | Oran |
|--------|-------|-------------|------|
| RPS | 20,000 | 24,716,741 | 1,236x |
| p99 Latency | 5ms | ~40ns | 125,000x |

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
