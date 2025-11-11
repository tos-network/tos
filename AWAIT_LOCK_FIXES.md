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

## 下一步

1. 运行自动修复工具
2. 检查自动修复的结果
3. 手动修复剩余问题
4. 运行完整测试套件
5. 提交 PR 合并到 main

## 参考

- Clippy 文档: https://rust-lang.github.io/rust-clippy/master/index.html#await_holding_lock
- Tokio 同步原语: https://docs.rs/tokio/latest/tokio/sync/
