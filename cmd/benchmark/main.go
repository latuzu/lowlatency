package main

import (
	"flag"
	"fmt"
	"log"
	"net/http"
	"sort"
	"sync"
	"sync/atomic"
	"time"
)

func main() {
	url := flag.String("url", "http://localhost:8080", "Base URL of the server")
	qps := flag.Int("qps", 10000, "Target queries per second")
	duration := flag.Int("duration", 10, "Test duration in seconds")
	keyCount := flag.Int64("keys", 1000000, "Number of keys to query (will cycle through key-0 to key-N)")
	flag.Parse()

	log.Printf("Starting benchmark:")
	log.Printf("  Target: %d QPS", *qps)
	log.Printf("  Duration: %d seconds", *duration)
	log.Printf("  Keys: 0 to %d", *keyCount-1)

	// Pre-generate keys to query
	keys := make([]string, *keyCount)
	for i := int64(0); i < *keyCount; i++ {
		keys[i] = fmt.Sprintf("key-%020d", i)
	}

	var (
		successCount int64
		errorCount   int64
		latencies    []time.Duration
		latenciesMu  sync.Mutex
	)

	client := &http.Client{
		Timeout: 5 * time.Second,
		Transport: &http.Transport{
			MaxIdleConns:        1000,
			MaxIdleConnsPerHost: 1000,
			IdleConnTimeout:     90 * time.Second,
		},
	}

	// Calculate sleep time between requests
	sleepTime := time.Second / time.Duration(*qps)

	startTime := time.Now()
	endTime := startTime.Add(time.Duration(*duration) * time.Second)

	// Launch workers
	var wg sync.WaitGroup
	keyIndex := int64(0)

	ticker := time.NewTicker(sleepTime)
	defer ticker.Stop()

	go func() {
		for now := range ticker.C {
			if now.After(endTime) {
				return
			}

			wg.Add(1)
			go func(idx int64) {
				defer wg.Done()

				key := keys[idx%*keyCount]
				reqURL := fmt.Sprintf("%s/get?key=%s", *url, key)

				reqStart := time.Now()
				resp, err := client.Get(reqURL)
				latency := time.Since(reqStart)

				if err != nil {
					atomic.AddInt64(&errorCount, 1)
					return
				}
				defer resp.Body.Close()

				if resp.StatusCode == http.StatusOK {
					atomic.AddInt64(&successCount, 1)
					latenciesMu.Lock()
					latencies = append(latencies, latency)
					latenciesMu.Unlock()
				} else {
					atomic.AddInt64(&errorCount, 1)
				}
			}(atomic.AddInt64(&keyIndex, 1))
		}
	}()

	time.Sleep(time.Duration(*duration) * time.Second)
	wg.Wait()

	// Calculate statistics
	totalRequests := successCount + errorCount
	actualDuration := time.Since(startTime)
	actualQPS := float64(totalRequests) / actualDuration.Seconds()

	sort.Slice(latencies, func(i, j int) bool {
		return latencies[i] < latencies[j]
	})

	var (
		p50, p95, p99, pMax time.Duration
		avgLatency          time.Duration
	)

	if len(latencies) > 0 {
		sum := time.Duration(0)
		for _, l := range latencies {
			sum += l
		}
		avgLatency = sum / time.Duration(len(latencies))

		p50 = latencies[len(latencies)*50/100]
		p95 = latencies[len(latencies)*95/100]
		p99 = latencies[len(latencies)*99/100]
		pMax = latencies[len(latencies)-1]
	}

	// Print results
	fmt.Println("\n=== Benchmark Results ===")
	fmt.Printf("Duration:        %.2f seconds\n", actualDuration.Seconds())
	fmt.Printf("Total Requests:  %d\n", totalRequests)
	fmt.Printf("Successful:      %d\n", successCount)
	fmt.Printf("Errors:          %d\n", errorCount)
	fmt.Printf("Actual QPS:      %.2f\n", actualQPS)
	fmt.Println("\n=== Latency ===")
	fmt.Printf("Average:         %.3f ms\n", float64(avgLatency.Microseconds())/1000)
	fmt.Printf("p50:             %.3f ms\n", float64(p50.Microseconds())/1000)
	fmt.Printf("p95:             %.3f ms\n", float64(p95.Microseconds())/1000)
	fmt.Printf("p99:             %.3f ms\n", float64(p99.Microseconds())/1000)
	fmt.Printf("Max:             %.3f ms\n", float64(pMax.Microseconds())/1000)

	if p99 <= 5*time.Millisecond {
		fmt.Println("\n✓ SUCCESS: p99 latency is within 5ms requirement!")
	} else {
		fmt.Println("\n✗ FAILED: p99 latency exceeds 5ms requirement!")
	}
}
