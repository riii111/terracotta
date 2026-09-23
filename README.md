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

In the review screen, press `s` to open the single-environment change overview.
Use `↑`/`↓` or `j`/`k` to select a row, `Space` to expand repeated changes,
`Enter` to open the corresponding raw plan block, and `/` to filter full
addresses. `Esc` returns to the same overview selection, while `v` opens the
full plan from its first line. Overview filtering changes display only; apply
and copy always use the complete reviewed plan.

When an interactive `plan` starts in a directory without configuration files,
Terracotta inspects its immediate child directories for backend or cloud
configuration. HCP candidates are excluded and invalid configurations are
reported. Local environments run one at a time in path order, using their current
workspace. Each environment initializes noninteractively when needed; missing
variables or initialization failures appear as `Error` while other plans continue.
The initial Overview shows resource rows and environment columns. Use `↑`/`↓`
to select a row, `←`/`→` to select an environment, and `Space` to expand a group.
`Enter` opens that resource's original plan; `1`–`9` open it in the corresponding
environment. Use `[`/`]` to reach any environment, including the tenth and later.
`Esc`, `0`, or `s` returns to Overview with its selection and expansion preserved.
`/` filters complete resource addresses; totals always cover the full plans.

Cells show `+`, `~`, `-`, `-/+`, or `+/-`; `.` means unchanged, a blank means absent,
and `?` means unavailable. Group counts describe a shared change pattern, not
one-to-one instance correspondence or resolved unknown values. `Compared` names
the Ready environments when acquisition is incomplete; excluded HCP environments
remain visible with their reason. Columns scroll horizontally as you select them.

Press `r` to retry only the selected `Error`, or `q` to stop acquisition and discard
the temporary plans. Multiple-environment apply is not supported.

This discovery path rejects `-out`, `-generate-config-out`, and a shared
`TF_DATA_DIR` before running commands. Relative `-var-file` paths resolve from the
parent directory. The exit code is 130 when interrupted, 1 if any environment has
an error or is excluded, and otherwise 2 for changes with `-detailed-exitcode`,
or 0 without it.

Try the cloudless demos: `python3 fixtures/basic/scenario.py demo` opens the
single-environment Overview. `python3 fixtures/environments/scenario.py demo`
opens the three-environment comparison. To see a repeated resource group from a
real plan, run `python3 fixtures/basic/scenario.py demo --scenario group-expansion`;
this downloads HashiCorp's time provider on first use, without connecting to a cloud.
In the plan review, press `s` for Overview, then select `[+] time_sleep.server[*]`
and press Space. To inspect the plan separately, run
`python3 fixtures/basic/scenario.py setup --scenario group-expansion`, then remove
the temporary directory with `python3 fixtures/basic/scenario.py clean PATH`.
Pass `--plugin-dir PATH` to use an existing local provider directory.
