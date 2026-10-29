# Popu

> 面向自动化脚本和 AI Agent 的本机通知管理与轻量交互窗口。

[English](README.en.md)

Popu 让脚本和 Agent 能够在需要时通知用户、请求确认、收集少量结构化输入，并在任务结束后保留可追溯的通知历史。它运行在本机，不需要网络服务；CLI 和 Python SDK 由常驻 Host 统一管理窗口、队列、状态、历史和托盘提醒。

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
- 本地 Named Pipe（Windows）或 Unix Domain Socket（Unix）；
- 不提供网络监听端口，不需要外部服务。

## 快速开始

以下命令在项目根目录运行。请先按照[构建与测试](#构建与测试)生成 `popu` 可执行文件，并将其所在目录加入 `PATH`。

### CLI

提交一个通知：

```powershell
popu request protocol/examples/notification-success.json
```

提交一个确认对话框：

```powershell
popu request protocol/examples/dialog-confirm.json
```

请求文件是 JSON，完整协议见 [JSON Schema](protocol/popu.schema.json) 和 [协议架构说明](ARCHITECTURE.md)。

### Python SDK

需要 Python 3.10 或更高版本，并确保 `popu` 可执行文件位于 `PATH`：

```powershell
python -m pip install -e ./python
```

发送通知：

```python
from popu import Client, Notification

client = Client()
receipt = client.notify(
    Notification.success(
        title="Backup completed",
        message="All files were backed up successfully.",
    )
)

print(receipt.request_id)
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

if result.status == "submitted":
    print("confirmed")
```

Python SDK 详细说明见 [python/README.md](python/README.md)。

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
- [Python SDK](python/README.md)
- [JSON Schema](protocol/popu.schema.json)
- [贡献指南](CONTRIBUTING.md)
- [安全说明](SECURITY.md)

## 构建与测试

当前主要在 Windows 10/11、Rust stable、Node.js 20+ 和 WebView2 Runtime 上验证。协议与 IPC 设计包含 Unix Domain Socket 路径，具体窗口能力取决于目标平台。

安装前端依赖并构建：

```powershell
npm ci
npm run build
```

Windows 可执行文件位于 `src-tauri/target/release/popu.exe`。将该目录加入 `PATH`，或直接使用此路径调用；Python 也可通过 `Client(executable="src-tauri/target/release/popu.exe")` 指定它。Python SDK 是 CLI 客户端，安装 SDK 不会安装 Rust 可执行文件。

运行测试：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
node --check ui/app.js
python -m pytest python/tests -q
```

开发期也可以分别构建 UI 和 Rust 可执行文件：

```powershell
npm run build:ui
cargo build --manifest-path src-tauri/Cargo.toml
```

欢迎通过 issue 和 pull request 参与改进，详见[贡献指南](CONTRIBUTING.md)。原生通知窗口生命周期测试见[通知窗口协调与排布](docs/notification-layout.md)；运行这些测试需要解锁的 Windows 桌面。

## 项目边界

Popu 是本机人机交互层，不是任务调度器、日志系统、远程推送服务或完整业务界面。调用方负责任务执行和业务数据，Popu 负责把需要用户看到或决定的摘要可靠地呈现出来。

项目当前处于持续演进阶段，除非需求明确要求，不承诺 CLI、SDK 和协议接口的向后兼容。

## 许可证

Apache License 2.0，详见 [LICENSE](LICENSE)。
