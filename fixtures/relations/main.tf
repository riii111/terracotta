terraform {
  required_version = ">= 1.4.0"
}

variable "choose_source" {
  type    = bool
  default = true
}

variable "root_only" {
  type    = string
  default = "root variable"
}

locals {
  unresolved = terraform_data.source.id
}

resource "terraform_data" "source" {
  input = "source"
}

resource "terraform_data" "other_source" {
  input = "other"
}

resource "terraform_data" "counted" {
  count = 2
  input = count.index
}

resource "terraform_data" "foreach" {
  for_each = {
    first  = terraform_data.source.input
    second = terraform_data.other_source.input
  }
  input = each.value
}

resource "terraform_data" "direct" {
  input = terraform_data.source.id
}

resource "terraform_data" "explicit" {
  input      = "explicit"
  depends_on = [terraform_data.source]
}

resource "terraform_data" "indexed" {
  input = terraform_data.counted[1].id
}

resource "terraform_data" "mixed" {
  input = "${terraform_data.source.id}:${local.unresolved}"
}

resource "terraform_data" "root_variable" {
  input = var.root_only
}

resource "terraform_data" "conditional" {
  input = var.choose_source ? terraform_data.source.id : terraform_data.other_source.id
}

resource "terraform_data" "count_expression" {
  count = length(terraform_data.source.input)
  input = count.index
}

resource "terraform_data" "foreach_expression" {
  for_each = { source = terraform_data.source.input }
  input    = each.value
}

module "child" {
  source = "./child"
  input  = terraform_data.source.id
}

resource "terraform_data" "module_dependent" {
  input      = "module dependent"
  depends_on = [module.child]
}

resource "terraform_data" "module_output_consumer" {
  input = module.child.output
}

resource "terraform_data" "merged_evidence" {
  input = terraform_data.source.id
}
