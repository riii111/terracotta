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
- **Clipboard**: Copy the full plan or apply results

## Usage

Run from your Terraform or OpenTofu configuration directory in an interactive terminal, with the selected tool and credentials already configured.

```sh
terracotta plan
```

`terracotta plan` runs Terraform's plan synchronously with the original
arguments. After a successful plan, Terracotta opens the review UI and exits
without applying anything.

```sh
terracotta apply
```

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

The review UI opens for interactive local `plan` and `apply` invocations,
including supported options and the selected tool's `TF_CLI_ARGS*`. CI, redirected
streams, HCP configurations, and unsupported options are delegated unchanged.
Terracotta does not run `init` implicitly.

In the review screen, press `s` to open the single-environment change overview.
Use `↑`/`↓` or `j`/`k` to select a row, `Space` to expand repeated changes,
`Enter` to open the corresponding raw plan block, and `/` to filter full
addresses. `Esc` returns to the same overview selection, while `v` opens the
full plan from its first line. Overview filtering changes display only; apply
and copy always use the complete reviewed plan.

When an interactive `plan` starts in a directory without configuration files,
Terracotta inspects its immediate child directories for backend or cloud
configuration. HCP candidates are excluded and invalid configurations are
reported. Multi-environment plan execution is not available yet; run from an
individual environment directory to review its plan. This discovery path rejects
`-out`, `-generate-config-out`, and a shared `TF_DATA_DIR` before running commands.
