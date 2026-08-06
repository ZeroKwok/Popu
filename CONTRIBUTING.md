# Contributing

欢迎提交 issue 和 pull request。修改应保持 Popu 的 v1 边界：无常驻服务、无持久化、一次调用对应一个独立窗口。

提交前请运行：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
node --check ui/app.js
python -m pytest python/tests -q
```

协议变更必须同步更新 JSON Schema、Rust 校验、Python 模型、fixture 和 `ARCHITECTURE.md`。新增控件必须由通用渲染器实现，不能通过请求或外部模板注入脚本。
