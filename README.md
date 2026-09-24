# Terracotta

Review and apply Terraform or OpenTofu plans in your terminal.

[![plan](https://github.com/user-attachments/assets/7c6bdb79-bf87-4f5f-9b33-7cafa038bc9e)


## Concept

> Terraform-native · Review before apply · Ephemeral UI

Review Terraform's familiar diff and apply the exact plan you reviewed. The UI appears when you need it, then returns you to your shell.

## Features

- **Plan review**: Full plan display, scrolling, and keyword filtering
- **Saved plan apply**: Target directory and workspace confirmation, with no replanning
- **Apply progress**: Resource status and its log stay visible together; `Previous` shows local successful history
- **Change overview**: Group repeated resource changes, filter by full address, and jump back to the matching raw plan block
- **Environment comparison**: Compare plans across detected child directories, with partial results and retry for failed environments
- **Clipboard**: Copy the full plan or apply results

## Usage

Run from your Terraform or OpenTofu configuration directory in an interactive
terminal, with the selected tool and credentials already configured. With no
arguments, Terracotta runs Terraform's plan and opens Overview; non-interactive
runs print help.

```sh
terracotta
terracotta plan
terracotta apply
terracotta tofu plan
```

`terracotta plan` runs Terraform's plan synchronously with the original
arguments. After a successful plan, Terracotta opens the full plan review and
exits without applying anything.

`terracotta apply` reviews the saved plan and asks for confirmation before
applying that exact plan. It does not re-plan after review. A plan that has no
changes exits successfully without showing an apply confirmation.

During apply, the upper panel lists resources and the lower panel shows the
selected resource's log. Use `↑`/`↓` or `j`/`k` to select a resource, `Tab` to
switch between the resource list and log, and select `All logs` to read output
that is not tied to a resource. The completed result keeps the counts and
elapsed time visible; `q` closes it and `y` copies the result and full log.

`terracotta terraform <arguments>` runs Terraform commands. `terracotta plan` and
`terracotta apply` are shortcuts.

`terracotta tofu <arguments>` runs OpenTofu commands. Use `alias tofu='terracotta tofu'`
to keep the usual command name. The Terraform equivalent is
`alias terraform='terracotta terraform'`.

For explicit commands, the review UI opens for supported interactive local
`plan` and `apply` invocations, including the selected tool's `TF_CLI_ARGS*`.
CI, redirected streams, HCP configurations, and unsupported options are
delegated unchanged. The no-argument entry prints help outside an interactive
terminal and reports unsupported backends or options without running a plan.
Single-environment commands do not run `init` implicitly.

In the review screen, press `s` to open the single-environment Overview. It has
`[2] Changes` and `[3] Relations` panes. Use `2`/`3` to focus a pane and `f` to
maximize or restore it. In Changes, use `↑`/`↓` or `j`/`k` to select a row,
`Space` to expand repeated changes, `/` to filter full addresses, and `Enter`
to open the corresponding raw plan block. Use `←`/`→` to scroll long addresses
horizontally. In Relations, use the arrow keys to scroll the graph; `Enter`
opens the raw plan from its first line. `Esc` returns from a raw plan to the
same Overview state. In Overview, `Esc` restores a maximized pane; if a
confirmed filter is active, it clears the filter after the split is restored.
`v` opens the full plan from its first line. Filtering changes display only;
apply and copy always use the complete reviewed plan.

When an interactive `plan` starts in a directory without configuration files,
Terracotta inspects its immediate child directories for backend or cloud
configuration. HCP candidates are excluded and invalid configurations are
reported. Local environments run one at a time in path order, using their current
workspace. Each environment initializes noninteractively when needed; missing
variables or initialization failures appear as `Error` while other plans continue.
The initial Overview has `[1] Envs`, `[2] Differs across envs`, and
`[3] Relations`. Use `1`/`2`/`3` to focus a pane and `f` to maximize or restore
it. In Envs, `↑`/`↓` selects an environment, `Space` includes or excludes it
from comparison, and `Enter` opens its original plan. Use `[`/`]` to switch the
selected environment from any pane, including reaching the tenth and later.
In Differs, `↑`/`↓` selects a row, `←`/`→` scrolls it horizontally, `Space`
expands a group, `/` filters complete resource addresses, and `Enter` opens the
selected resource's plan. In Relations, `↑`/`↓` scrolls vertically and
`←`/`→` scrolls horizontally; `Enter` opens the selected environment's plan
from its first line. From a raw plan, `Esc` returns to the comparison Overview
with its selection and expansion preserved. Totals always cover the full plans.

Cells show `+`, `~`, `-`, `-/+`, or `+/-`; `.` means unchanged, a blank means absent,
and `?` means unavailable. Group counts describe a shared change pattern, not
one-to-one instance correspondence or resolved unknown values. The `[2]` title
shows the compared environment count as `Filtered x/y` when some environments
are excluded, and the Ready count as `Ready x/y` while acquisition is incomplete.
Excluded HCP environments remain visible with their reason.

Press `r` to retry only the selected `Error`, or `q` to stop acquisition and discard
the temporary plans. Multiple-environment apply is not supported.

This discovery path rejects `-out`, `-generate-config-out`, and a shared
`TF_DATA_DIR` before running commands. Relative `-var-file` paths resolve from the
parent directory. The exit code is 130 when interrupted, 1 if any environment has
an error or is excluded, and otherwise 2 for changes with `-detailed-exitcode`,
or 0 without it.

Try the cloudless demos:

```sh
python3 fixtures/demo.py single
python3 fixtures/demo.py multi
```

`single` opens the single-environment plan. `multi` opens the `dev`, `prod`, and
`stg` comparison. Both commands prepare a temporary local scenario, build
Terracotta, and remove the scenario when the review exits. The automatic fixture
acceptance checks also validate repeated-resource expansion and multi-environment
retry with both Terraform and OpenTofu.
