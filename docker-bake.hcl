group "default" {
  targets = ["customer-service", "message-service", "facebook-graph-service", "test-runner"]
}

target "customer-service" {
  context = "."
  dockerfile = "services/customer-service/Dockerfile"
  tags = ["fbpage-mm-bridge/customer-service:test"]
}

target "message-service" {
  context = "."
  dockerfile = "services/message-service/Dockerfile"
  tags = ["fbpage-mm-bridge/message-service:test"]
}

target "facebook-graph-service" {
  context = "."
  dockerfile = "services/facebook-graph-service/Dockerfile"
  tags = ["fbpage-mm-bridge/facebook-graph-service:test"]
}

target "test-runner" {
  context = "."
  dockerfile = "Dockerfile.test"
  tags = ["fbpage-mm-bridge/test-runner:test"]
}
