# ast-grep checks

`ast-grep` 0.42.1 checks repository source syntax. The current Rust test rules cover `src/**/*.rs`, `tests/**/*.rs`, and `examples/**/*.rs`. Each rule declares its exact file scope in `.ast-grep/rules/`, and its matching file in `.ast-grep/tests/` defines the valid and invalid cases. Those YAML files are the source of truth for rule-specific behavior. This README covers the shared workflow, not a catalog of every rule; adding a rule does not require updating it unless the shared scope, limits, or workflow change.

For example, a test condition belongs on a module declaration:

```rust
#[cfg(test)]
mod tests {
    fn helper() {}
}
```

The same condition on an individual function is rejected:

```rust
#[cfg(test)]
fn helper() {}
```

A named rstest case such as `#[case::up_arrow(Key::Up)]` is allowed on a function; an unnamed `#[case(Key::Up)]` is rejected. The `#[case]` marker on a function argument is allowed.

These checks operate on parsed source syntax. They do not expand macros, resolve modules across files, or judge whether a helper belongs to a particular test module or whether a case name explains its input. The test-condition rule also excludes `cfg_attr`, `cfg!`, inner `#![cfg(...)]` attributes, and attributes containing only the `test-support` feature. The rules have no automatic fixes because moving helpers can change visibility and dependencies.

From the repository root, run `ast-grep test --skip-snapshot-tests` to validate the rule cases, then `ast-grep scan` to check source files. The pre-commit hook runs the scan; install Lefthook and ast-grep 0.42.1 in your environment and enable the repository hook with `lefthook install`. The Nix devShell provides both tools. CI runs the rule tests and scan.

To add or change a rule, edit `.ast-grep/rules/`, add valid and invalid examples in the matching `.ast-grep/tests/` file, and rerun both commands.
