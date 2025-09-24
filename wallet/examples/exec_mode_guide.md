# TOS Wallet Exec Mode Guide

TOS Wallet 现在完全采用与 Geth 一致的 `--exec` 模式，提供简洁、强大的批处理操作。

## 🚀 **执行模式对比**

### Geth 风格
```bash
# Geth 执行 JavaScript 代码
geth --exec "eth.getBalance(eth.accounts[0])" console
geth --exec "personal.sendTransaction({...}, 'password')" console
```

### TOS Wallet 风格 (完全对齐)
```bash
# TOS Wallet 执行简单命令
tos_wallet --exec="balance TOS" --wallet-path="wallet" --password="pass"
tos_wallet --exec="transfer TOS tos1abc... 100" --wallet-path="wallet" --password="pass"
```

## 📋 **支持的执行方式**

### 1. **--exec** (主要方式，与 Geth 对齐)

#### 基础查询命令
```bash
# 查询余额
tos_wallet --exec="balance TOS" --wallet-path="my_wallet" --password="secret"

# 获取地址
tos_wallet --exec="address" --wallet-path="my_wallet" --password="secret"

# 查看 nonce
tos_wallet --exec="nonce" --wallet-path="my_wallet" --password="secret"
```

#### 钱包管理命令
```bash
# 设置 nonce
tos_wallet --exec="set_nonce 42" --wallet-path="my_wallet" --password="secret"

# 跟踪资产
tos_wallet --exec="track_asset abc123..." --wallet-path="my_wallet" --password="secret"

# 取消跟踪资产
tos_wallet --exec="untrack_asset abc123..." --wallet-path="my_wallet" --password="secret"
```

#### 交易命令
```bash
# 转账交易
tos_wallet --exec="transfer TOS tos1abc... 100" --wallet-path="my_wallet" --password="secret"

# 转账所有余额
tos_wallet --exec="transfer_all TOS tos1abc..." --wallet-path="my_wallet" --password="secret"

# 销毁代币
tos_wallet --exec="burn TOS 50" --wallet-path="my_wallet" --password="secret"
```

### 2. **--json** (高级配置)

适用于需要复杂参数的场景：

```bash
# 结构化转账配置
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

# 冻结 TOS 配置
tos_wallet --json='{
  "command": "freeze_tos",
  "params": {
    "amount": "1000",
    "duration": 7,
    "confirm": "yes"
  }
}' --wallet-path="my_wallet" --password="secret"
```

### 3. **--json-file** (批量配置)

适用于复杂的批量操作：

```bash
# 从文件执行配置
tos_wallet --json-file="transfer_config.json" --wallet-path="my_wallet" --password="secret"
```

配置文件示例 (`transfer_config.json`):
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

## 🎯 **使用场景推荐**

| 场景 | 推荐方式 | 示例 |
|------|----------|------|
| **简单查询** | `--exec` | `--exec="balance TOS"` |
| **基础操作** | `--exec` | `--exec="set_nonce 42"` |
| **复杂交易** | `--json` | JSON 字符串配置 |
| **批量脚本** | `--json-file` | 配置文件 |
| **CI/CD 集成** | `--exec` + `--json-file` | 混合使用 |

## 🔧 **脚本集成示例**

### Bash 脚本
```bash
#!/bin/bash
WALLET="my_wallet"
PASSWORD="secret123"

# 检查余额
echo "Current balance:"
tos_wallet --exec="balance TOS" --wallet-path="$WALLET" --password="$PASSWORD"

# 执行转账
echo "Sending transaction:"
tos_wallet --exec="transfer TOS tos1recipient... 10" --wallet-path="$WALLET" --password="$PASSWORD"
```

### Python 集成
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

# 使用示例
balance_output, _ = run_wallet_command("balance TOS", "my_wallet", "secret")
print(f"Balance: {balance_output}")
```

## ⚡ **性能和便利性**

### 优势
1. **简洁语法**: 一行命令完成操作
2. **与 Geth 一致**: 熟悉的接口，降低学习成本
3. **类型安全**: JSON 参数提供编译时检查
4. **灵活性**: 支持简单命令和复杂配置

### 最佳实践
1. **日常使用**: 优先使用 `--exec`
2. **复杂场景**: 使用 `--json` 或 `--json-file`
3. **脚本自动化**: 根据复杂度选择合适的方式
4. **错误处理**: 检查命令返回状态码

## 🆚 **与其他工具对比**

| 工具 | 执行方式 | TOS Wallet 等效 |
|------|----------|----------------|
| `geth --exec "cmd"` | JavaScript | `--exec="cmd"` |
| `bitcoin-cli cmd` | 单命令 | `--exec="cmd"` |
| `solana cmd` | 子命令 | `--exec="cmd"` |

TOS Wallet 的 `--exec` 模式结合了各种工具的优点，提供了一致、强大且易用的接口！