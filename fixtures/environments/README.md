# Three local environments

This scenario uses local state and the built-in `terraform_data` resource. No
provider download, cloud account, or remote backend is needed. All files are
created in a new temporary directory.

For a ready-to-review three-environment demo, run:

```sh
python3 fixtures/environments/scenario.py demo
```

It builds Terracotta, opens the `dev`, `prod`, and `stg` comparison with every
environment Ready, and removes the temporary scenario when the review exits.
Use `--tool tofu` to run the demo with OpenTofu.

```sh
scenario_dir=$(python3 fixtures/environments/scenario.py setup)
terracotta terraform -chdir="$scenario_dir" plan
```

Terracotta detects `dev`, `prod`, and `stg` in that order and initializes each
one. `dev` and `stg` have updates; `dev` also has a create. `prod` fails because
its required `release` variable is unset. The Overview compares Ready plans
while retaining the Error column. Enter opens a resource's original plan;
Esc returns to the selected cell.

From another terminal, supply the missing variable:

```sh
python3 fixtures/environments/scenario.py repair "$scenario_dir"
```

Select the `prod` column and press `r`. Only `prod` runs again; all three plans
become Ready. Press `q` to exit, then remove the entire scenario:

```sh
python3 fixtures/environments/scenario.py clean "$scenario_dir"
```

For OpenTofu, pass `--tool tofu` to `setup` and use
`terracotta tofu -chdir="$scenario_dir" plan`.

Run the complete PTY acceptance against an already-built binary:

```sh
python3 fixtures/environments/acceptance.py --binary /absolute/path/to/terracotta --tool terraform
python3 fixtures/environments/acceptance.py --binary /absolute/path/to/terracotta --tool tofu
```

The acceptance pauses `prod` while reviewing `dev`, repairs the missing variable,
retries only `prod`, and checks command counts, unchanged local state, plan cleanup,
and terminal restoration. It removes its temporary directory on exit.
