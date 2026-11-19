//go:build linux

// Package main does things.
package main

/*
#include <stdio.h>

int add(int a, int b) { return a + b; }
*/
import "C"

import (
	"dev.azure.com/foo/bar/pkg/some"
)

/* single line block comment */
import "dev.azure.com/foo/bar/pkg/retry"

func main() {}

const fake = `
import "dev.azure.com/foo/bar/pkg/ignored"
`
