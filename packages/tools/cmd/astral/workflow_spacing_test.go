package main

import "testing"

func TestSpaceWorkflowSeparatesJobsAndSteps(t *testing.T) {
	input := `name: ci

jobs:
  build:
    steps:
      - uses: actions/checkout@v4
        with:
          persist-credentials: false
      - name: Build
        run: |
          echo one
          - not a step
      # a comment
      - run: echo two
  test:
    runs-on: ubuntu-latest
    steps:
      - run: echo three
`
	want := `name: ci

jobs:
  build:
    steps:
      - uses: actions/checkout@v4
        with:
          persist-credentials: false

      - name: Build
        run: |
          echo one
          - not a step

      # a comment
      - run: echo two

  test:
    runs-on: ubuntu-latest
    steps:
      - run: echo three
`
	if got := spaceWorkflow(input); got != want {
		t.Fatalf("spaceWorkflow:\n%s", got)
	}
	if again := spaceWorkflow(want); again != want {
		t.Fatalf("spaceWorkflow is not stable:\n%s", again)
	}
}
