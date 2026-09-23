terraform {
  required_providers {
    time = {
      source  = "hashicorp/time"
      version = "= 0.14.2"
    }
  }
}

resource "time_sleep" "server" {
  count = 2

  create_duration = "1s"
  destroy_duration = "1s"
}
