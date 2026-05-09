// Package basic is a test fixture: covers interfaces, goroutines, packages.
package basic

import (
	"fmt"
	"sync"
)

// Reader is a minimal interface — exercises interface analysis.
type Reader interface {
	Read(p []byte) (int, error)
}

// MemReader is a struct that satisfies Reader.
type MemReader struct {
	data []byte
	pos  int
}

func NewMemReader(data []byte) *MemReader {
	return &MemReader{data: data, pos: 0}
}

// Read implements Reader.
func (r *MemReader) Read(p []byte) (int, error) {
	if r.pos >= len(r.data) {
		return 0, fmt.Errorf("eof")
	}
	n := copy(p, r.data[r.pos:])
	r.pos += n
	return n, nil
}

// FanOutSquares spawns goroutines and returns the squares concurrently.
func FanOutSquares(nums []int) []int {
	out := make([]int, len(nums))
	var wg sync.WaitGroup
	for i, n := range nums {
		wg.Add(1)
		go func(idx, val int) {
			defer wg.Done()
			out[idx] = val * val
		}(i, n)
	}
	wg.Wait()
	return out
}
