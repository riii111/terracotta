# Terracotta

Review and apply Terraform plans in your terminal.

[![plan](https://github.com/user-attachments/assets/7c6bdb79-bf87-4f5f-9b33-7cafa038bc9e)


## Concept

> Terraform-native · Review before apply · Ephemeral UI

Review Terraform's familiar diff and apply the exact plan you reviewed. The UI appears when you need it, then returns you to your shell.

## Features

- **Plan review**: Full plan display, scrolling, and keyword filtering
- **Saved plan apply**: Target directory and workspace confirmation, with no replanning
- **Clipboard**: Copy the full plan or apply results

## Usage

Run from your Terraform configuration directory in an interactive terminal, with Terraform and credentials already configured.

```sh
terracotta plan
```

`init → plan → review → apply (optional)`

`terracotta terraform <arguments>` passes Terraform commands through; `terracotta plan`
and `terracotta apply` are shortcuts. Terraform help and version flags belong after
`terraform`, while `terracotta --help` and `terracotta --version` describe Terracotta.

Currently, the review UI accepts only an interactive `plan` without options.
Commands with options, `apply`, CI, redirected streams, and HCP configurations run
Terraform directly with the original arguments and environment. HCP configurations
that only store state remotely are also passed through.
