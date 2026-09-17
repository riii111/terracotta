terraform {
  required_version = ">= 1.4.0"
}

resource "terraform_data" "api" {
  input = "v1"
}

resource "terraform_data" "worker" {
  triggers_replace = "v1"
}

resource "terraform_data" "old" {
  input = "delete-me"
}
