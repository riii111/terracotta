variable "input" {
  type = string
}

resource "terraform_data" "inside" {
  input = var.input
}

output "output" {
  value = terraform_data.inside.id
}
