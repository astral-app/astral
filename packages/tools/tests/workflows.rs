use astral_tools::workflows::space_workflow;

#[test]
fn separates_jobs_and_steps() {
    let input = "name: ci\n\njobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          persist-credentials: false\n      - name: Build\n        run: |\n          echo one\n          - not a step\n      # a comment\n      - run: echo two\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo three\n";
    let want = "name: ci\n\njobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          persist-credentials: false\n\n      - name: Build\n        run: |\n          echo one\n          - not a step\n\n      # a comment\n      - run: echo two\n\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo three\n";
    assert_eq!(space_workflow(input), want);
    assert_eq!(space_workflow(want), want);
}
