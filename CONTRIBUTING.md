# Contributing

欢迎提交 issue 和 pull request。修改应遵循当前单实例常驻 Host 架构：CLI 与 Python SDK 是客户端，Host 统一管理请求、窗口和队列；仅通知终态写入本机历史，dialog 及其表单值不持久化。

提交前请运行：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
node --check ui/app.js
python -m pytest python/tests -q
```

协议变更必须同步更新 JSON Schema、Rust 校验、Python 模型、fixture 和 `ARCHITECTURE.md`。新增控件必须由通用渲染器实现，不能通过请求或外部模板注入脚本。
