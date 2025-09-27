# TOS Wallet Exec Mode Guide

TOS Wallet now fully adopts the `--exec` mode consistent with Geth, providing concise and powerful batch operations.

## 🚀 **Execution Mode Comparison**

### Geth Style
```bash
# Geth executes JavaScript code
geth --exec "eth.getBalance(eth.accounts[0])" console
geth --exec "personal.sendTransaction({...}, 'password')" console
```

### TOS Wallet Style (Fully Aligned)
```bash
# TOS Wallet executes simple commands
tos_wallet --exec="balance TOS" --wallet-path="wallet" --password="pass"
tos_wallet --exec="transfer TOS tos1abc... 100" --wallet-path="wallet" --password="pass"
```

## 📋 **Supported Execution Methods**

### 1. **--exec** (Primary Method, Aligned with Geth)

#### Basic Query Commands
```bash
# Query balance
tos_wallet --exec="balance TOS" --wallet-path="my_wallet" --password="secret"

# Get address
tos_wallet --exec="address" --wallet-path="my_wallet" --password="secret"

# Check nonce
tos_wallet --exec="nonce" --wallet-path="my_wallet" --password="secret"
```

#### Wallet Management Commands
```bash
# Set nonce
tos_wallet --exec="set_nonce 42" --wallet-path="my_wallet" --password="secret"

# Track asset
tos_wallet --exec="track_asset abc123..." --wallet-path="my_wallet" --password="secret"

# Untrack asset
tos_wallet --exec="untrack_asset abc123..." --wallet-path="my_wallet" --password="secret"
```

#### Transaction Commands
```bash
# Transfer transaction
tos_wallet --exec="transfer TOS tos1abc... 100" --wallet-path="my_wallet" --password="secret"

# Transfer all balance
tos_wallet --exec="transfer_all TOS tos1abc..." --wallet-path="my_wallet" --password="secret"

# Burn tokens
tos_wallet --exec="burn TOS 50" --wallet-path="my_wallet" --password="secret"
```

### 2. **--json** (Advanced Configuration)

Suitable for scenarios requiring complex parameters:

```bash
# Structured transfer configuration
tos_wallet --json='{
  "command": "transfer",
  "params": {
    "address": "tos1abc...",
    "amount": "100.5",
    "asset": "TOS",
    "fee_type": "low",
    "confirm": "yes"
  }
}' --wallet-path="my_wallet" --password="secret"

# Freeze TOS configuration
tos_wallet --json='{
  "command": "freeze_tos",
  "params": {
    "amount": "1000",
    "duration": 7,
    "confirm": "yes"
  }
}' --wallet-path="my_wallet" --password="secret"
```

### 3. **--json-file** (Batch Configuration)

Suitable for complex batch operations:

```bash
# Execute configuration from file
tos_wallet --json-file="transfer_config.json" --wallet-path="my_wallet" --password="secret"
```

Configuration file example (`transfer_config.json`):
```json
{
  "command": "transfer",
  "params": {
    "address": "tos1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq8cczjp",
    "amount": "500",
    "asset": "TOS",
    "confirm": "yes"
  }
}
```

## 🎯 **Usage Scenario Recommendations**

| Scenario | Recommended Method | Example |
|----------|-------------------|---------|
| **Simple Queries** | `--exec` | `--exec="balance TOS"` |
| **Basic Operations** | `--exec` | `--exec="set_nonce 42"` |
| **Complex Transactions** | `--json` | JSON string configuration |
| **Batch Scripts** | `--json-file` | Configuration files |
| **CI/CD Integration** | `--exec` + `--json-file` | Hybrid usage |

## 🔧 **Script Integration Examples**

### Bash Script
```bash
#!/bin/bash
WALLET="my_wallet"
PASSWORD="secret123"

# Check balance
echo "Current balance:"
tos_wallet --exec="balance TOS" --wallet-path="$WALLET" --password="$PASSWORD"

# Execute transfer
echo "Sending transaction:"
tos_wallet --exec="transfer TOS tos1recipient... 10" --wallet-path="$WALLET" --password="$PASSWORD"
```

### Python Integration
```python
import subprocess

def run_wallet_command(exec_cmd, wallet_path, password):
    cmd = [
        "tos_wallet",
        "--exec", exec_cmd,
        "--wallet-path", wallet_path,
        "--password", password
    ]

    result = subprocess.run(cmd, capture_output=True, text=True)
    return result.stdout, result.stderr

# Usage example
balance_output, _ = run_wallet_command("balance TOS", "my_wallet", "secret")
print(f"Balance: {balance_output}")
```

## ⚡ **Performance and Convenience**

### Advantages
1. **Concise Syntax**: Complete operations with one command
2. **Consistent with Geth**: Familiar interface, reducing learning curve
3. **Type Safety**: JSON parameters provide compile-time checking
4. **Flexibility**: Supports both simple commands and complex configurations

### Best Practices
1. **Daily Usage**: Prioritize using `--exec`
2. **Complex Scenarios**: Use `--json` or `--json-file`
3. **Script Automation**: Choose appropriate method based on complexity
4. **Error Handling**: Check command return status codes

## 🆚 **Comparison with Other Tools**

| Tool | Execution Method | TOS Wallet Equivalent |
|------|------------------|----------------------|
| `geth --exec "cmd"` | JavaScript | `--exec="cmd"` |
| `bitcoin-cli cmd` | Single command | `--exec="cmd"` |
| `solana cmd` | Subcommand | `--exec="cmd"` |

TOS Wallet's `--exec` mode combines the advantages of various tools, providing a consistent, powerful, and user-friendly interface!