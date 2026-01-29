package main

import (
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
)

func createTestDataFile(tb testing.TB, count int) string {
	tmpfile, err := os.CreateTemp("", "test_data_*.bin")
	if err != nil {
		tb.Fatalf("Failed to create temp file: %v", err)
	}
	filename := tmpfile.Name()

	buffer := make([]byte, RecordSize)
	for i := 0; i < count; i++ {
		key := fmt.Sprintf("key-%020d", i)
		value := fmt.Sprintf("value-%d", i)
		
		copy(buffer[:KeySize], []byte(key))
		for j := len(key); j < KeySize; j++ {
			buffer[j] = 0
		}
		
		copy(buffer[KeySize:], []byte(value))
		for j := KeySize + len(value); j < RecordSize; j++ {
			buffer[j] = 0
		}
		
		_, err := tmpfile.Write(buffer)
		if err != nil {
			tb.Fatalf("Failed to write record: %v", err)
		}
	}
	
	tmpfile.Close()
	return filename
}

func TestStore_LoadAndGet(t *testing.T) {
	filename := createTestDataFile(t, 100)
	defer os.Remove(filename)

	store, err := NewStore(filename)
	if err != nil {
		t.Fatalf("Failed to create store: %v", err)
	}
	defer store.Close()

	if store.count != 100 {
		t.Errorf("Expected count=100, got %d", store.count)
	}

	// Test getting first key
	value, found := store.Get("key-00000000000000000000")
	if !found {
		t.Error("Expected to find key-00000000000000000000")
	}
	expectedValue := "value-0"
	if string(value) != expectedValue {
		t.Errorf("Expected value=%s, got %s", expectedValue, string(value))
	}

	// Test getting last key
	value, found = store.Get("key-00000000000000000099")
	if !found {
		t.Error("Expected to find key-00000000000000000099")
	}
	expectedValue = "value-99"
	if string(value) != expectedValue {
		t.Errorf("Expected value=%s, got %s", expectedValue, string(value))
	}

	// Test non-existent key
	_, found = store.Get("nonexistent")
	if found {
		t.Error("Should not find nonexistent key")
	}
}

func TestStore_EmptyFile(t *testing.T) {
	tmpfile, err := os.CreateTemp("", "empty_*.bin")
	if err != nil {
		t.Fatalf("Failed to create temp file: %v", err)
	}
	filename := tmpfile.Name()
	tmpfile.Close()
	defer os.Remove(filename)

	store, err := NewStore(filename)
	if err != nil {
		t.Fatalf("Failed to create store from empty file: %v", err)
	}
	defer store.Close()

	if store.count != 0 {
		t.Errorf("Expected count=0 for empty file, got %d", store.count)
	}
}

func TestStore_InvalidFileSize(t *testing.T) {
	tmpfile, err := os.CreateTemp("", "invalid_*.bin")
	if err != nil {
		t.Fatalf("Failed to create temp file: %v", err)
	}
	filename := tmpfile.Name()
	
	// Write invalid size (not multiple of RecordSize)
	tmpfile.Write(make([]byte, 100))
	tmpfile.Close()
	defer os.Remove(filename)

	_, err = NewStore(filename)
	if err == nil {
		t.Error("Expected error for invalid file size")
	}
}

func TestHTTPHandlers(t *testing.T) {
	filename := createTestDataFile(t, 10)
	defer os.Remove(filename)

	store, err := NewStore(filename)
	if err != nil {
		t.Fatalf("Failed to create store: %v", err)
	}
	defer store.Close()

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

		w.Header().Set("Content-Type", "application/octet-stream")
		w.Header().Set("Content-Length", fmt.Sprintf("%d", len(value)))
		w.WriteHeader(http.StatusOK)
		if _, err := w.Write(value); err != nil {
			t.Logf("Error writing response: %v", err)
		}
	})

	mux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain")
		w.WriteHeader(http.StatusOK)
		if _, err := fmt.Fprintf(w, "OK - %d records loaded\n", store.count); err != nil {
			t.Logf("Error writing health response: %v", err)
		}
	})

	t.Run("GET with valid key", func(t *testing.T) {
		req := httptest.NewRequest("GET", "/get?key=key-00000000000000000000", nil)
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, req)

		if w.Code != http.StatusOK {
			t.Errorf("Expected status 200, got %d", w.Code)
		}

		body, _ := io.ReadAll(w.Body)
		expected := "value-0"
		if string(body) != expected {
			t.Errorf("Expected body=%s, got %s", expected, string(body))
		}
	})

	t.Run("GET with missing key parameter", func(t *testing.T) {
		req := httptest.NewRequest("GET", "/get", nil)
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, req)

		if w.Code != http.StatusBadRequest {
			t.Errorf("Expected status 400, got %d", w.Code)
		}
	})

	t.Run("GET with non-existent key", func(t *testing.T) {
		req := httptest.NewRequest("GET", "/get?key=nonexistent", nil)
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, req)

		if w.Code != http.StatusNotFound {
			t.Errorf("Expected status 404, got %d", w.Code)
		}
	})

	t.Run("POST not allowed", func(t *testing.T) {
		req := httptest.NewRequest("POST", "/get?key=key-00000000000000000000", nil)
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, req)

		if w.Code != http.StatusMethodNotAllowed {
			t.Errorf("Expected status 405, got %d", w.Code)
		}
	})

	t.Run("Health endpoint", func(t *testing.T) {
		req := httptest.NewRequest("GET", "/health", nil)
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, req)

		if w.Code != http.StatusOK {
			t.Errorf("Expected status 200, got %d", w.Code)
		}

		body, _ := io.ReadAll(w.Body)
		expected := "OK - 10 records loaded\n"
		if string(body) != expected {
			t.Errorf("Expected body=%s, got %s", expected, string(body))
		}
	})
}

func BenchmarkStore_Get(b *testing.B) {
	filename := createTestDataFile(b, 1000)
	defer os.Remove(filename)

	store, err := NewStore(filename)
	if err != nil {
		b.Fatalf("Failed to create store: %v", err)
	}
	defer store.Close()

	key := "key-00000000000000000500"
	
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, found := store.Get(key)
		if !found {
			b.Fatal("Key not found")
		}
	}
}

func BenchmarkHTTPHandler_Get(b *testing.B) {
	filename := createTestDataFile(b, 1000)
	defer os.Remove(filename)

	store, err := NewStore(filename)
	if err != nil {
		b.Fatalf("Failed to create store: %v", err)
	}
	defer store.Close()

	mux := http.NewServeMux()
	mux.HandleFunc("/get", func(w http.ResponseWriter, r *http.Request) {
		key := r.URL.Query().Get("key")
		value, found := store.Get(key)
		if !found {
			http.Error(w, "Key not found", http.StatusNotFound)
			return
		}
		w.Header().Set("Content-Type", "application/octet-stream")
		w.Header().Set("Content-Length", fmt.Sprintf("%d", len(value)))
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(value)  // Ignore error in benchmark
	})

	req := httptest.NewRequest("GET", "/get?key=key-00000000000000000500", nil)
	
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, req)
		if w.Code != http.StatusOK {
			b.Fatal("Request failed")
		}
	}
}
