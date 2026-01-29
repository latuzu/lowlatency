package main

import (
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"syscall"
	"time"
)

const (
	KeySize    = 64
	ValueSize  = 256
	RecordSize = KeySize + ValueSize // 320 bytes per record
)

type Store struct {
	data   []byte
	index  map[string]int64 // key -> offset in data
	count  int64
}

func NewStore(filename string) (*Store, error) {
	// Open the data file
	file, err := os.Open(filename)
	if err != nil {
		return nil, fmt.Errorf("failed to open file: %w", err)
	}
	defer file.Close()

	// Get file size
	info, err := file.Stat()
	if err != nil {
		return nil, fmt.Errorf("failed to stat file: %w", err)
	}
	
	fileSize := info.Size()
	if fileSize%RecordSize != 0 {
		return nil, fmt.Errorf("invalid file size: %d (not a multiple of %d)", fileSize, RecordSize)
	}

	// Memory-map the file for zero-copy reads
	data, err := syscall.Mmap(int(file.Fd()), 0, int(fileSize), syscall.PROT_READ, syscall.MAP_SHARED)
	if err != nil {
		return nil, fmt.Errorf("failed to mmap file: %w", err)
	}

	count := fileSize / RecordSize
	
	// Build hash index
	index := make(map[string]int64, count)
	for i := int64(0); i < count; i++ {
		offset := i * RecordSize
		key := string(data[offset : offset+KeySize])
		// Trim null bytes from key
		for j := 0; j < len(key); j++ {
			if key[j] == 0 {
				key = key[:j]
				break
			}
		}
		index[key] = offset
	}

	log.Printf("Loaded %d records, built index with %d keys", count, len(index))

	return &Store{
		data:  data,
		index: index,
		count: count,
	}, nil
}

func (s *Store) Get(key string) ([]byte, bool) {
	offset, exists := s.index[key]
	if !exists {
		return nil, false
	}
	
	// Return value directly from mmap'd memory (zero-copy)
	valueOffset := offset + KeySize
	value := s.data[valueOffset : valueOffset+ValueSize]
	
	// Find actual value length (trim null bytes)
	length := ValueSize
	for i := 0; i < ValueSize; i++ {
		if value[i] == 0 {
			length = i
			break
		}
	}
	
	return value[:length], true
}

func (s *Store) Close() error {
	if s.data != nil {
		return syscall.Munmap(s.data)
	}
	return nil
}

func main() {
	dataFile := flag.String("data", "data.bin", "Path to data file")
	port := flag.Int("port", 8080, "HTTP server port")
	flag.Parse()

	// Load the store
	store, err := NewStore(*dataFile)
	if err != nil {
		log.Fatalf("Failed to load store: %v", err)
	}
	defer store.Close()

	// Setup HTTP handlers with optimizations
	mux := http.NewServeMux()
	
	mux.HandleFunc("/get", func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}

		key := r.URL.Query().Get("key")
		if key == "" {
			http.Error(w, "Missing key parameter", http.StatusBadRequest)
			return
		}

		value, found := store.Get(key)
		if !found {
			http.Error(w, "Key not found", http.StatusNotFound)
			return
		}

		// Set headers for optimal performance
		w.Header().Set("Content-Type", "application/octet-stream")
		w.Header().Set("Content-Length", fmt.Sprintf("%d", len(value)))
		w.WriteHeader(http.StatusOK)
		w.Write(value)
	})

	mux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain")
		w.WriteHeader(http.StatusOK)
		fmt.Fprintf(w, "OK - %d records loaded\n", store.count)
	})

	// Configure server for low latency and high throughput
	server := &http.Server{
		Addr:           fmt.Sprintf(":%d", *port),
		Handler:        mux,
		ReadTimeout:    1 * time.Second,
		WriteTimeout:   1 * time.Second,
		IdleTimeout:    60 * time.Second,
		MaxHeaderBytes: 1 << 16, // 64KB
	}

	log.Printf("Server starting on port %d with %d records", *port, store.count)
	log.Fatal(server.ListenAndServe())
}
