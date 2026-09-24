resource "terraform_data" "state_target" {
  input = "state target"
}

resource "terraform_data" "state_dependent" {
  input = terraform_data.state_target.id
}
