# Contributing

欢迎提交 issue 和 pull request。修改应遵循当前单实例常驻 Host 架构：CLI 与 Python SDK 是客户端，Host 统一管理请求、窗口和队列；仅通知终态写入本机历史，dialog 及其表单值不持久化。

提交前请运行：

```powershell
just setup-ui
just icons
just check
just test-visuals
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

根目录 just 负责跨项目编排。UI 源码、图表入口、npm 配置和前端测试位于 `ui/`，生成到 `ui/dist/`；Cargo 只读取这个产物。`scripts/` 保留跨项目打包和原生 smoke 工具。首次检出需安装 ImageMagick 并生成原生图标；Python 测试与构建工具请在开发虚拟环境中安装 pytest 和 build。

产品版本唯一来源为 `src-tauri/Cargo.toml` 的 `package.version`，不要手工编辑生成的 `python/src/popu/_version.py`。Python 构建、editable 安装和 `just test-python` 都会自动准备版本；直接使用 `PYTHONPATH` 加载源码时先执行 `just prepare-python`。Tauri 省略版本字段以继承 Cargo；UI npm 包不声明产品版本。应用发行版本与 JSON/RPC 协议版本独立。

协议变更必须同步更新 JSON Schema、Rust 校验、Python 模型、fixture 和 `ARCHITECTURE.md`。新增控件必须由通用渲染器实现，不能通过请求或外部模板注入脚本。
