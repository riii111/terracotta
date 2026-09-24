# Three-environment demo and acceptance

Run the multi-environment TUI demo from the repository root:

```sh
python3 fixtures/demo.py multi
```

The demo prepares `dev`, `prod`, and `stg` with local state and the built-in
`terraform_data` resource. All three environments start Ready. The script builds
Terracotta and removes the temporary directories after the TUI exits.

Use `↑`/`↓` to select a resource row and `←`/`→` to select an environment.
`Enter` opens the original plan; `Esc` or `s` returns to Overview. Press `q` to
exit.

The cloudless PTY acceptance exercises an initial `prod` error, repairs its
missing variable, retries only `prod`, and checks plan cleanup, unchanged state,
and terminal restoration against the real command-line binary:

```sh
cargo build --locked
python3 fixtures/environments/acceptance.py --binary target/debug/terracotta --tool terraform
python3 fixtures/environments/acceptance.py --binary target/debug/terracotta --tool tofu
```
