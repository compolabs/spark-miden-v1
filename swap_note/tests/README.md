
## Running Full Integration Tests

```bash
# This will ensure we start from a clean node and client
make clean-node
# This command will clone the node's repo and generate the accounts and genesis files
make node
# This command will run the node
make start-node
# This will run the integration test 
make integration-test-full
```

## Running Mock Integration Tests

```bash
cargo test --test mock_integration
```
