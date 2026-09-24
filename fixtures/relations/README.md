# Relationship evidence fixture

This cloudless scenario checks Terraform's real `show -json` and `state pull` shapes used by GR01. It applies only built-in `terraform_data` resources in a temporary directory, removes a pair of resources from configuration, then checks that their saved-plan and state evidence remains available. Generated plans, state, and JSON stay outside the repository and are removed when the script exits.

Run with either supported CLI:

```sh
python3 fixtures/relations/scenario.py --tool terraform
python3 fixtures/relations/scenario.py --tool tofu
```

Synthetic malformed and mixed-evidence cases are covered by the Rust parser tests; this scenario verifies the real CLI format without printing plan or state contents.
