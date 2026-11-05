# 贡献指南

欢迎提交 issue 和 pull request。修改应遵循当前单实例常驻 Host 架构：CLI 与 Python SDK 是客户端，Host 统一管理请求、窗口和队列；仅通知终态写入本机历史，dialog 及其表单值不持久化。

提交前请运行：

```powershell
just setup
just check
just test-visuals
just fmt-check
```

根目录 just 负责跨项目编排。UI 源码、图表入口、npm 配置和前端测试位于 `ui/`，生成到 `ui/dist/`；Cargo 只读取这个产物。`scripts/` 保留跨项目打包和原生 smoke 工具。首次检出需安装 ImageMagick 并生成原生图标；Python 测试与构建工具请在开发虚拟环境中安装 pytest、packaging、build 和 twine。

`just` 默认只显示帮助；配方按 Project、UI、Rust、Python、Demo 分组。`check` 包含无人工交互的工具、SDK、UI 和 Rust 测试；`fmt-check` 单独检查 Rust 格式。需要人操作或展示窗口的示例使用 `demo-*`，原生 smoke 显式运行 `test-native`。旧的 `test <fixture>`、`build-test`、`build-test-release` 分别改为 `demo-fixture`、`demo-fixtures`、`demo-fixture-release`；空前缀表示全部示例，不再提供冗余的 `build-test-all`。

跨项目逻辑集中在 `scripts/dev.py` 和 `scripts/project_paths.py`，测试位于 `scripts/tests/`，通过 `just test-tools` 运行。程序路径由 Cargo metadata 提供，不硬编码构建目录；`PYTHON` 可指定解释器路径。`prep-native` 按需生成缺失/过期图标，`icons` 强制生成。justfile 不固定全局 shell；现有 wheel 和原生 smoke 配方显式标注 Windows 平台。

产品版本唯一来源为 `src-tauri/Cargo.toml` 的 `package.version`，不要手工编辑生成的 `python/src/popu/_version.py`。Python 构建、editable 安装和 `just test-python` 都会自动准备版本；直接使用 `PYTHONPATH` 加载源码时先执行 `just prep-python`。Tauri 省略版本字段以继承 Cargo；UI npm 包不声明产品版本。应用发行版本与 JSON/RPC 协议版本独立。

`just build-wheel` 自动准备固定版本的 `cargo-about`：优先复用 PATH、项目缓存或 Cargo 安装目录中的匹配版本，缺失时安装到 `python/build/tools/`，不要求手动安装或修改全局工具。首次安装需要联网和编译时间。wheel 使用独立 Windows target 和静态 CRT，并生成与 Cargo.lock 对应的 Rust 依赖通知；普通开发 release exe 不作为 wheel 的默认输入。发行检查与 Trusted Publisher 配置见 [Python 分发](docs/python-distribution.md)。发布工作流手动运行只生成审核制品，发布 GitHub Release 才会上传 PyPI。

协议变更必须同步更新 JSON Schema、Rust 校验、Python 模型、fixture 和 `ARCHITECTURE.md`。新增控件必须由通用渲染器实现，不能通过请求或外部模板注入脚本。

## 文档语言

项目文档默认使用中文，原路径保留为中文入口；已有英文版使用同名的 `.en.md` 文件，并在页面顶部提供双向语言切换。涉及双语文档的内容变更应在同一次修改中同步，以中文版为准。

文档中的命令、API 名称、路径、协议字段和示例输出保持原样，控制台提示继续使用英文。英文导航优先指向已有英文版；目标仅有中文时应明确标注。翻译标题后应同步检查链接锚点。用于 PyPI 介绍的 Python README 使用完整 GitHub URL 提供语言切换，确保仓库外也能访问。
