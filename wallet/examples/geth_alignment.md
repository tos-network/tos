# TOS Wallet Alignment with Geth Commands

TOS Wallet now supports a Geth-like `--exec` argument, making batch operations more intuitive and consistent.

## 🆚 **Command Comparison**

### Geth Approach
```bash
# Query balance
geth --exec "eth.getBalance(eth.accounts[0])" console

# Send transaction
geth --exec "personal.sendTransaction({from:'0x...', to:'0x...', value: web3.toWei(1, 'ether')}, 'password')" console

# Get block info
geth --exec "eth.getBlock('latest')" console
```

### TOS Wallet Approach (After Alignment)
```bash
# Query balance
tos_wallet --exec="balance TOS" --wallet-path="my_wallet" --password="secret"

# Send transaction
tos_wallet --exec="transfer TOS tos1abc... 100" --wallet-path="my_wallet" --password="secret"

# Get address
tos_wallet --exec="address" --wallet-path="my_wallet" --password="secret"
```

## 🚀 **Supported Execution Modes**

### 1. **--exec** (Aligned with Geth)
```bash
# Simple commands
tos_wallet --exec="balance TOS" --wallet-path="wallet" --password="pass"
tos_wallet --exec="address" --wallet-path="wallet" --password="pass"
tos_wallet --exec="set_nonce 42" --wallet-path="wallet" --password="pass"

# Complex commands
tos_wallet --exec="transfer TOS tos1abc... 100" --wallet-path="wallet" --password="pass"
```

### 2. **--json** (TOS Wallet Enhancements)
```bash
# JSON string
tos_wallet --json='{"command":"balance","params":{"asset":"TOS"}}' \
    --wallet-path="wallet" --password="pass"

# Structured transfer
tos_wallet --json='{"command":"transfer","params":{"address":"tos1...","amount":"100","asset":"TOS","confirm":"yes"}}' \
    --wallet-path="wallet" --password="pass"
```

### 3. **--json-file** (Batch Configuration)
```bash
# Execute from file
tos_wallet --json-file="batch_config.json" --wallet-path="wallet" --password="pass"
```

## 📋 **Use Case Comparison**

| Scenario | Geth | TOS Wallet |
|------|------|------------|
| **Simple queries** | `--exec "eth.getBalance(...)"` | `--exec "balance TOS"` |
| **Complex operations** | JavaScript code | JSON configuration |
| **Script integration** | Requires JavaScript knowledge | Simple CLI arguments |
| **Batch operations** | Write script files | JSON file configuration |
| **Type safety** | Runtime checks | Compile-time checks |

## 🎯 **Benefits**

### Improvements in TOS Wallet
1. **Simpler**: No complex JavaScript syntax needed
2. **Type-safe**: JSON parameters provide type checking
3. **Backward compatible**: Still supports `--batch-mode` (deprecated but available)
4. **Flexible configuration**: Supports both file-based and inline configuration

### Consistency with Geth
1. **`--exec` argument**: Execute command and exit directly
2. **Non-interactive mode**: Suitable for scripts and automation
3. **Simple syntax**: Accomplish tasks with a one-liner

## 📖 **Migration Guide**

### Migrate from legacy --batch-mode to --exec

**Old way (still supported)**:
```bash
tos_wallet --batch-mode --cmd="balance TOS" --wallet-path="wallet" --password="pass"
```

**New way (recommended)**:
```bash
tos_wallet --exec="balance TOS" --wallet-path="wallet" --password="pass"
```

### Priority
1. `--exec` (newest, recommended)
2. `--json` / `--json-file` (for complex scenarios)
3. `--batch-mode --cmd` (backward compatible, deprecated)

This design keeps TOS Wallet consistent with Geth while offering more powerful and flexible capabilities!