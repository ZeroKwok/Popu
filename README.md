# Popu

> 面向自动化脚本和 AI Agent 的本机通知管理与轻量交互窗口。

[English](README.en.md)

Popu 让脚本和 Agent 能够在需要时通知用户、请求确认、收集少量结构化输入，并在任务结束后保留可追溯的通知历史。它运行在本机，不需要网络服务。通过 pip 一次安装即可获得 Python SDK 和 `popu` 命令行工具。

## 快速开始

### 安装并运行演示

**目前仅支持 Windows x64，需要 64 位 Python 3.10+ 和 [Microsoft WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。** 如缺少 WebView2，程序会提示手动安装入口。

```sh
python -m pip install popu
python -m popu.demo progress
```

演示展示任务进度和最终摘要，可以尝试中途关闭进度卡片，观察后台更新和最后一次摘要展示。它使用模拟备份数据，不会操作你的文件；按 Ctrl+C 可停止演示。可加 `--wait` 等待最终通知结束展示，控制台提示使用英文。

安装包已包含匹配的原生程序，无需检出仓库或安装编译工具，可在任意目录使用。后台 Host 会在首次请求时自动启动。

### Python SDK

发送通知：

```python
from popu import Client, Notification

Client().notify(
    Notification.success(
        title="Backup completed",
        message="All files were backed up successfully.",
    )
)
```

请求用户确认：

```python
from popu import Client, DialogRequest

result = Client().show(
    DialogRequest.confirm(
        title="Deploy release?",
        message="Production deployment is ready.",
    )
)

if result.status == "submitted" and result.action == "confirm":
    print("Deployment confirmed")
```

进度更新示例见 [Python 入门](python/README.md#update-task-progress)，完整接口见 [SDK 参考](docs/python-sdk.md)。

### CLI

```sh
popu --help
popu templates list
```

将以下内容保存为 `notification.json`，即可从任意目录提交通知：

```json
{
  "version": "1.0",
  "mode": "notify",
  "template": "notification",
  "content": {
    "severity": "success",
    "title": "Task completed",
    "blocks": [{"type": "paragraph", "text": "Your results are ready."}]
  }
}
```

```sh
popu request notification.json
```

如果 `popu` 不在 `PATH`，使用 `python -m popu` 代替。更多请求见 [JSON Schema](protocol/popu.schema.json) 和[协议架构说明](ARCHITECTURE.md)。

## 为什么使用 Popu

自动化任务通常需要把结果告诉人，或者在继续执行前等待一个明确决定。Popu 适合以下场景：

- 备份、构建、部署等任务完成后发送通知；
- AI Agent 执行高风险操作前请求确认；
- 长时间任务展示进度、关键事实和结果摘要；
- 多个脚本同时运行时统一管理通知；
- 通知超时或提交进程退出后，仍可在历史中查找结果。

## 核心能力

### 通知管理

- 常驻 Host 统一管理通知生命周期；
- 通知严格 FIFO 排队，主显示器工作区最多显示三条；
- 支持提交回执、状态查询、完整内容更新、撤销和等待终态；
- 支持 `coalesce_key`，避免同一个业务位置重复创建通知；
- 支持通知历史、未读状态和托盘提醒；
- 通知始终置顶，但不会主动抢占当前应用焦点；
- 提交 CLI 或 Python 进程退出后，通知仍由 Host 独立管理。

### 轻量交互

- `dialog` 确认对话框和轻量表单；
- 结构化 action、表单值、终态和退出码；
- 段落、属性、列表、进度、图片和 Vega-Lite 图表；
- 内容更新和 revision 冲突检测；
- 无边框、透明的本机窗口，尺寸由请求配置。

### 集成方式

- JSON CLI；
- Python SDK；
- 本地 IPC，由后台 Host 统一管理窗口与历史；
- 不提供网络监听端口，不需要外部服务。

## 工作方式

```text
CLI / Python SDK
        │
        ▼
Persistent Popu Host
        │
        ├── notification queue
        ├── dialog queue
        ├── request registry
        ├── notification history
        └── tray reminder
```

Host 在首次请求时自动启动。调用方提交请求后获得 Host 生成的 `request_id` 和 revision：

| CLI 入口或请求模式 | 行为 |
| --- | --- |
| `popu request <file>`，`mode: "notify"` | 返回接受回执，通知由 Host 继续管理 |
| `popu request <file>`，`mode: "dialog"` | 输出接受回执，再等待并输出确认、取消、关闭或超时等终态的响应 |
| `popu update <id> <update.json>` | 使用最新 revision 替换完整 `content` |
| `popu dismiss <id>` | 撤销排队或展示中的请求 |
| `popu status <id>` | 查询请求当前完整快照 |
| `popu wait <id>` | 等待请求进入终态并读取 Host 快照 |

通知的 `duration_ms` 从内容准备完成并正式展示后开始计算。dialog 的终态和结构化响应不会依赖提交进程继续存活；通知在提交进程退出后也不会消失。

通知终态会保存到历史；dialog 及其表单值不会持久化。只有 `requires_read: true` 且仍未阅读的历史通知会持续触发托盘提醒。

长时间任务创建通知时声明 `updatable: true`，使用同一 ID 持续更新；最后一次更新设为 `updatable: false`。用户关闭后普通更新静默保存，`reshow: true` 默认允许最后再次展示一次；自动超时后的下次更新会正常再次展示。每次成功更新都重新计时，并可通过 `duration_ms` 覆盖展示时长，默认模板显示通知发生时间。连续 12 小时未上报会自动过期归档。完整规则见[可更新通知](docs/live-notifications.md)，可运行的备份演示见[Python 示例](python/src/popu/demo/README.md)。

## 通知与窗口规则

- 通知使用主显示器工作区，最多同时展示三条；
- 通知窗口始终 `always_on_top: true`，显式提交 `false` 会被拒绝；
- 通知默认不抢占焦点；
- dialog 可独立配置屏幕、工作区或物理屏幕、尺寸和是否置顶；
- 宽高支持逻辑像素或百分比，不需要根据 DPI 手动换算；
- 通知和 dialog 都支持可复制文本、图片和图表；
- 大量日志、完整报告和复杂编辑界面应留在调用方产品中，Popu 负责展示摘要和必要输入。

## 文档导航

- [English README](README.en.md)
- [完整架构说明](ARCHITECTURE.md)
- [通知窗口协调与排布](docs/notification-layout.md)
- [可更新通知生命周期](docs/live-notifications.md)
- [Python 安装与入门](python/README.md)
- [Python SDK 参考](docs/python-sdk.md)
- [Python 包构建与发布](docs/python-distribution.md)
- [JSON Schema](protocol/popu.schema.json)
- [贡献指南](CONTRIBUTING.md)
- [安全说明](SECURITY.md)

## 构建与测试

当前主要在 Windows 10/11、Rust stable、Node.js 20+、just 和 WebView2 Runtime 上验证。根目录 just 编排任务，`ui/` 管理 npm 项目，`src-tauri/` 管理 Cargo 项目，`python/` 管理 SDK 与 wheel。界面源码位于 `ui/src/`，Cargo 只嵌入 `ui/dist/` 构建产物。协议与 IPC 设计包含 Unix Domain Socket 路径，具体窗口能力取决于目标平台。

首次检出需要安装 ImageMagick。安装前端依赖、准备生成文件并构建：

```sh
just setup
just build-release
```

Windows 可执行文件位于 `src-tauri/target/release/popu.exe`。将该目录加入 `PATH`，或直接使用此路径调用。源码开发可执行 `python -m pip install -e ./python`，并通过 `Client(executable="src-tauri/target/release/popu.exe")` 指定本地构建；editable 安装不会构建或下载二进制。正式 Windows wheel 则同时包含原生可执行文件与 SDK。

运行测试：

```sh
just check
# 浏览器视觉回归（不会启动原生 Host）：
just test-visuals
```

开发期也可以分别构建 UI 和 Rust 可执行文件：

```sh
just build-ui
cargo build --manifest-path src-tauri/Cargo.toml
```

产品版本只修改 `src-tauri/Cargo.toml` 中的 `package.version`。Tauri 自动读取它，Python 构建和 editable 安装自动生成 SDK 版本文件；`just version` 显示当前版本，`just build-wheel` 构建包含原生程序的本地 wheel。Python 测试需要 pytest，wheel 构建需要 build；请在开发用虚拟环境中安装。

直接运行 `just` 会显示分组帮助。自动化检查使用 `just check`，交互演示使用 `just demo-fixture notification-success.json`、`just demo-fixtures notification` 或 `just demo-python --wait`。演示与 wheel 载荷准备通过 Cargo metadata 获取程序路径，支持 `CARGO_TARGET_DIR`；可用 `PYTHON` 环境变量指定 Python 解释器。构建会按需生成缺失或过期的图标，`just icons` 可强制重建。`just build-wheel` 和现有 `just test-native` 目前仅在 Windows 提供。

欢迎通过 issue 和 pull request 参与改进，详见[贡献指南](CONTRIBUTING.md)。原生通知窗口生命周期测试见[通知窗口协调与排布](docs/notification-layout.md)；运行这些测试需要解锁的 Windows 桌面。

## 项目边界

Popu 是本机人机交互层，不是任务调度器、日志系统、远程推送服务或完整业务界面。调用方负责任务执行和业务数据，Popu 负责把需要用户看到或决定的摘要可靠地呈现出来。

项目当前处于持续演进阶段，除非需求明确要求，不承诺 CLI、SDK 和协议接口的向后兼容。

## 许可证

Apache License 2.0，详见 [LICENSE](LICENSE)。
