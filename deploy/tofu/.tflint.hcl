# tflint for deploy/tofu (DEC-2012 item 2): the bundled terraform ruleset only, so no plugin is
# downloaded, with every rule of its recommended preset and the naming and documentation rules.

config {
  call_module_type = "local"
}

plugin "terraform" {
  enabled = true
  preset  = "recommended"
}

rule "terraform_naming_convention" {
  enabled = true
}

rule "terraform_unused_declarations" {
  enabled = true
}
