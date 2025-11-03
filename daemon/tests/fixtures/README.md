# Test Fixtures for TAKO VM Integration Tests

This directory contains compiled TAKO VM contracts used for integration testing.

## Files

- `hello_world.so` - Simple hello-world contract for basic execution tests

## Updating Test Contracts

If you need to rebuild the test contracts from source:

```bash
# Build hello-world contract
cd ../../../tako/examples/hello-world
./build.sh

# Copy to test fixtures
cp target/tbpf-tos-tos/release/hello_world.so \
   ../../../tos/daemon/tests/fixtures/
```

## Why Fixtures?

The test contracts are copied here to avoid requiring a full TAKO rebuild every time tests are run. This makes the test suite faster and more reliable.

## Adding New Test Contracts

When adding new test contracts:

1. Build the contract in the tako/examples directory
2. Copy the compiled `.so` file to this fixtures directory
3. Update the integration tests to use the new fixture
4. Update this README with the new contract description
