# await_holding_lock 问题修复计划

## 问题统计

- **总数**: 192 个 `await_holding_lock` 警告
- **继承自 Xelis**: 74 个
- **TOS 新增**: 118 个 ⚠️

## 风险等级: 🔴 高危

持有锁时进行 `.await` 操作可能导致：
- 死锁
- 性能严重下降
- 并发竞争问题
- 区块链同步失败

## 修复策略

### Phase 1: 自动修复（优先尝试）

```bash
# 尝试自动修复
cargo clippy --fix --allow-dirty --allow-staged -- -W clippy::await_holding_lock
```

### Phase 2: 手动修复模式

#### 模式 A: 提前释放锁

```rust
// ❌ 错误
let data = lock.lock().unwrap();
some_async_fn().await;
drop(data);

// ✅ 正确
let data = {
    let data = lock.lock().unwrap();
    data.clone()
}; // 锁在这里自动释放
some_async_fn().await;
```

#### 模式 B: 使用 async-aware 锁

```rust
// ❌ 错误: 使用 std::sync::Mutex
use std::sync::Mutex;
let lock = Mutex::new(data);

// ✅ 正确: 使用 tokio::sync::Mutex
use tokio::sync::Mutex;
let lock = Mutex::new(data);
let guard = lock.lock().await;
some_async_fn().await;
```

#### 模式 C: 缩小锁作用域

```rust
// ❌ 错误: 锁作用域太大
let guard = lock.lock().unwrap();
let value = guard.get_value();
let result = process(value).await;

// ✅ 正确: 只在必要时持有锁
let value = {
    let guard = lock.lock().unwrap();
    guard.get_value().clone()
};
let result = process(value).await;
```

## 预计工作量

- **Phase 1 自动修复**: 可能修复 30-50% (60-96 个)
- **Phase 2 手动修复**: 剩余 96-132 个
- **总时间**: 3-5 天

## 自动修复结果 ❌

尝试了 `cargo clippy --fix` 但失败了：
- Clippy 尝试修复但引入了编译错误
- 错误：泛型参数移除导致类型不匹配
- 结论：**必须手动修复**

## 手动修复策略

### 优先级排序

1. **高优先级**: TOS 新增的 118 个问题（最近的代码）
2. **中优先级**: 继承自 Xelis 的 74 个问题

### 具体执行步骤

1. ✅ 创建修复分支并推送
2. ✅ 尝试自动修复（失败）
3. ⏳ 手动逐个修复（需要 3-5 天）
4. ⏳ 每修复一批，运行测试验证
5. ⏳ 所有修复完成后提交 PR
6. ⏳ 代码审查和合并到 main

## 下一步行动

**建议**: 由于需要手动修复 192 个问题，建议分阶段进行：

### Phase 1: 修复最关键的模块（1-2 天）
- `daemon/src/core/blockchain.rs` - 区块链核心
- `daemon/src/core/mempool.rs` - 交易池
- `daemon/src/rpc/rpc.rs` - RPC 接口

### Phase 2: 修复 TAKO 相关（1 天）
- `daemon/src/tako_integration/` - TAKO VM 集成

### Phase 3: 修复其他模块（1-2 天）
- 其余 daemon 和 wallet 模块

每个 Phase 完成后提交一次，便于增量审查。

## 参考

- Clippy 文档: https://rust-lang.github.io/rust-clippy/master/index.html#await_holding_lock
- Tokio 同步原语: https://docs.rs/tokio/latest/tokio/sync/
