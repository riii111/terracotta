terraform {
  required_version = ">= 1.4.0"
}

resource "terraform_data" "api" {
  input = "v2"
}

resource "terraform_data" "worker" {
  triggers_replace = "v2"
}

resource "terraform_data" "new" {
  input = "create-me"
}
