package main

import (
	"crypto/rand"
	"flag"
	"fmt"
	"log"
	"os"
)

const (
	KeySize    = 64
	ValueSize  = 256
	RecordSize = KeySize + ValueSize
)

func generateRandomBytes(n int) []byte {
	b := make([]byte, n)
	rand.Read(b)
	return b
}

func main() {
	count := flag.Int64("count", 1000000, "Number of records to generate")
	output := flag.String("output", "data.bin", "Output file path")
	flag.Parse()

	log.Printf("Generating %d records...", *count)

	file, err := os.Create(*output)
	if err != nil {
		log.Fatalf("Failed to create file: %v", err)
	}
	defer file.Close()

	buffer := make([]byte, RecordSize)
	
	for i := int64(0); i < *count; i++ {
		// Generate key: use a pattern that's easier to query
		key := fmt.Sprintf("key-%020d", i)
		copy(buffer[:KeySize], []byte(key))
		// Pad rest of key with zeros
		for j := len(key); j < KeySize; j++ {
			buffer[j] = 0
		}

		// Generate random value
		value := generateRandomBytes(ValueSize)
		copy(buffer[KeySize:], value)

		_, err := file.Write(buffer)
		if err != nil {
			log.Fatalf("Failed to write record %d: %v", i, err)
		}

		if (i+1)%1000000 == 0 {
			log.Printf("Generated %d records...", i+1)
		}
	}

	log.Printf("Successfully generated %d records to %s", *count, *output)
	log.Printf("File size: %d bytes (%.2f GB)", (*count)*RecordSize, float64(*count)*RecordSize/1024/1024/1024)
}
