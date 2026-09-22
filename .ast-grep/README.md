# ast-grep

Rules live in `.ast-grep/rules/`; valid and invalid examples live in `.ast-grep/tests/`.

The Nix devShell provides ast-grep and Lefthook. From the repository root:

```sh
ast-grep test --skip-snapshot-tests
ast-grep scan
lefthook install # Enable the pre-commit hook (once).
```

The hook scans source files; CI runs both rule tests and the scan. Outside the devShell, use the ast-grep version pinned in `.github/workflows/ci.yml`.

Checks use source syntax; they do not expand macros or resolve modules across files.
