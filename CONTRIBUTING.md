# Contributing

欢迎提交 issue 和 pull request。修改应遵循当前单实例常驻 Host 架构：CLI 与 Python SDK 是客户端，Host 统一管理请求、窗口和队列；仅通知终态写入本机历史，dialog 及其表单值不持久化。

提交前请运行：

```powershell
just setup
just check
just test-visuals
just fmt-check
```

根目录 just 负责跨项目编排。UI 源码、图表入口、npm 配置和前端测试位于 `ui/`，生成到 `ui/dist/`；Cargo 只读取这个产物。`scripts/` 保留跨项目打包和原生 smoke 工具。首次检出需安装 ImageMagick 并生成原生图标；Python 测试与构建工具请在开发虚拟环境中安装 pytest 和 build。

`just` 默认只显示帮助；配方按 Project、UI、Rust、Python、Demo 分组。`check` 包含无人工交互的工具、SDK、UI 和 Rust 测试；`fmt-check` 单独检查 Rust 格式。需要人操作或展示窗口的示例使用 `demo-*`，原生 smoke 显式运行 `test-native`。旧的 `test <fixture>`、`build-test`、`build-test-release` 分别改为 `demo-fixture`、`demo-fixtures`、`demo-fixture-release`；空前缀表示全部示例，不再提供冗余的 `build-test-all`。

跨项目逻辑集中在 `scripts/dev.py` 和 `scripts/project_paths.py`，测试位于 `scripts/tests/`，通过 `just test-tools` 运行。程序路径由 Cargo metadata 提供，不硬编码构建目录；`PYTHON` 可指定解释器路径。`prep-native` 按需生成缺失/过期图标，`icons` 强制生成。justfile 不固定全局 shell；现有 wheel 和原生 smoke 配方显式标注 Windows 平台。

产品版本唯一来源为 `src-tauri/Cargo.toml` 的 `package.version`，不要手工编辑生成的 `python/src/popu/_version.py`。Python 构建、editable 安装和 `just test-python` 都会自动准备版本；直接使用 `PYTHONPATH` 加载源码时先执行 `just prep-python`。Tauri 省略版本字段以继承 Cargo；UI npm 包不声明产品版本。应用发行版本与 JSON/RPC 协议版本独立。

协议变更必须同步更新 JSON Schema、Rust 校验、Python 模型、fixture 和 `ARCHITECTURE.md`。新增控件必须由通用渲染器实现，不能通过请求或外部模板注入脚本。
