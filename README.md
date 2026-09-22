# Terracotta

Review and apply Terraform or OpenTofu plans in your terminal.

[![plan](https://github.com/user-attachments/assets/7c6bdb79-bf87-4f5f-9b33-7cafa038bc9e)


## Concept

> Terraform-native · Review before apply · Ephemeral UI

Review Terraform's familiar diff and apply the exact plan you reviewed. The UI appears when you need it, then returns you to your shell.

## Features

- **Plan review**: Full plan display, scrolling, and keyword filtering
- **Saved plan apply**: Target directory and workspace confirmation, with no replanning
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

`terracotta terraform <arguments>` runs Terraform commands. `terracotta plan` and
`terracotta apply` are shortcuts.

`terracotta tofu <arguments>` runs OpenTofu commands. Use `alias tofu='terracotta tofu'`
to keep the usual command name. The Terraform equivalent is
`alias terraform='terracotta terraform'`.

The review UI opens for interactive local `plan` and `apply` invocations,
including supported options and the selected tool's `TF_CLI_ARGS*`. CI, redirected
streams, HCP configurations, and unsupported options are delegated unchanged.
Terracotta does not run `init` implicitly.
