# ast-grep checks

`ast-grep` checks repository source syntax. The current Rust test rules cover `src/**/*.rs`, `tests/**/*.rs`, and `examples/**/*.rs`. Each rule declares its exact file scope in `.ast-grep/rules/`, and its matching file in `.ast-grep/tests/` defines the valid and invalid cases. Those YAML files are the source of truth for rule-specific behavior. This README covers the shared workflow, not a catalog of every rule; adding a rule does not require updating it unless the shared scope, limits, or workflow change.

For example, a test condition belongs on a `tests` module declaration. Helpers and dormant test-only modules live beneath that entry without another `cfg(test)` attribute:

```rust
#[cfg(test)]
mod tests {
    mod support;
    use self::support::fixture;
}
```

The same condition on an individual function or a differently named module is rejected:

```rust
#[cfg(test)]
fn helper() {}

#[cfg(test)]
mod support;
```

The rule also catches `#[cfg(not(test))]` and compound conditions containing `test`. A `not(test)` item belongs to production code; keep it outside `tests` and reconsider selecting production implementations by test build mode.

A named rstest case such as `#[case::up_arrow(Key::Up)]` is allowed on a function; an unnamed `#[case(Key::Up)]` is rejected. The `#[case]` marker on a function argument is allowed.

These checks operate on parsed source syntax. They do not expand macros, resolve modules across files, verify that helpers are declared only once, or judge whether a case name explains its input. Share a helper by defining it once under the nearest common `tests::support` and importing it from each consumer. The test-condition rule excludes `cfg_attr`, `cfg!`, inner `#![cfg(...)]` attributes, and attributes containing only the separate cross-crate `test-support` feature. The rules have no automatic fixes because moving helpers can change visibility and dependencies.

From the repository root, run `ast-grep test --skip-snapshot-tests` to validate the rule cases, then `ast-grep scan` to check source files. The pre-commit hook runs the scan; the Nix devShell provides Lefthook and ast-grep. Enable the repository hook with `lefthook install`. CI installs its pinned ast-grep version through `install-action` and runs the rule tests and scan.

To add or change a rule, edit `.ast-grep/rules/`, add valid and invalid examples in the matching `.ast-grep/tests/` file, and rerun both commands.
