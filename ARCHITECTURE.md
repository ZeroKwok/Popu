# Popu 架构设计

> 状态：Prototype / Draft 0.2  
> 首发平台：Windows 10/11  
> 建议许可证：Apache-2.0

## 1. 定位

**Popu** 是供自动化、Agent 和脚本调用的本机交互器：需要人决定时弹出一个小窗口，或仅显示一条完成/异常通知。

它不是任务系统、工作流引擎、通知中心或远程审批平台。v1 的边界刻意保持很小：一个调用对应一次 UI 会话；调用结束后不保存任务、不支持稍后恢复，也不连接 Slack、邮件、网页或其他设备。

```text
Python / Shell / automation
          │ request JSON
          ▼
       popu.exe ──► Tauri + WebView2 window ──► user
          ▲                                            │
          └──────── response JSON（仅 dialog）─────────┘
```

## 2. 目标与非目标

### 2.1 v1 目标

- Windows 上提供轻量、漂亮、置顶的 WebView 交互窗。
- 提供 CLI 和 Python SDK；Python SDK 只封装 CLI，不另建服务端。
- 支持等待响应的 `dialog`，以及无需响应的 `notify`。
- 内置 HTML 模板，且允许调用方选择或替换**受信任的本地模板**。
- 支持右下角展示，或在鼠标所在屏幕的中心展示。
- 请求/响应使用版本化 JSON；AI 输出仅作为数据传入模板。
- 尽量将 HTML/CSS/JS 打包进发布物；Windows 使用 Evergreen WebView2 Runtime。

### 2.2 明确不做

- 不持久化请求、响应、待办任务或审计记录。
- 不支持调用进程崩溃后的恢复、查询、重试、跨设备同步或多人审批。
- 不启动常驻守护进程、不使用 SQLite、不开放网络端口或本地 IPC 服务。
- 不支持远程模板 URL、模型下发任意 HTML/JavaScript、iframe 或模板中的任意系统访问。
- 不在 v1 支持 macOS/Linux；设计中保留未来迁移到 Tauri 对应 WebView 的空间。

## 3. 使用模型

### 3.1 对话框：`dialog`

`dialog` 用于确认、选择、表单输入和人工反馈。调用方启动 Popu 并等待，用户提交、取消、关闭窗口或超时后，Popu 通过 stdout 返回一个 JSON 响应并退出。

```text
caller ── spawn popu dialog --wait ──► user action ──► JSON response ──► caller
```

这是短生命周期同步会话：调用方退出时，Popu 应结束该会话并关闭窗口；Popu 崩溃则调用方得到运行错误。业务端必须把 `cancelled`、`closed` 和 `timed_out` 当作正常且安全的结果处理。

### 3.2 通知：`notify`

`notify` 用于“任务执行完成”“操作失败”“发现可处理事项”等只需传达、不等待决定的场景。

```text
caller ── popu notify ──► Popu 确认窗口已创建 ──► caller 立即继续
                              │
                              └── 用户关闭 / duration 到期后退出
```

为避免调用方退出导致通知消失，CLI 启动一个短生命周期的 detached UI 子进程；父进程只等待“窗口创建成功/失败”的握手。通知不落盘，机器重启或 Popu 被关闭后自然消失。

## 4. CLI 与 Python API

### 4.1 CLI

```text
popu dialog --request request.json --wait
popu notify --request notification.json
popu templates list
popu templates validate --path .\templates\my-template
```

- `dialog --wait`：stdout 仅输出一个 `InteractionResponse` JSON；stderr 输出诊断日志。
- `notify`：窗口成功创建后立即返回；stdout 输出 `{ "status": "shown" }`。
- 退出码：`0` 请求已正常完成/显示；`10` 用户取消或关闭；`11` 对话框超时；`12` 请求、模板或运行时错误。

### 4.2 Python SDK

Python 包名建议为 `popu`，仅负责构造和校验 JSON、调用已安装的 `popu.exe`、解析返回值。它不嵌入 Tauri，也不自行实现窗口。

```python
from popu import DialogRequest, Notification, Position, show, notify

result = show(DialogRequest.confirm(
    title="Replace files?",
    message="3 existing files will be replaced.",
    position=Position.CURSOR_SCREEN_CENTER,
))

if result.status == "submitted" and result.values["confirmed"]:
    replace_files()

notify(Notification.success(
    title="Backup completed",
    message="Details: 128 files, 2.4 GB, 03:21.",
    position=Position.BOTTOM_RIGHT,
))
```

同步 API 是 v1 必需项；`asyncio` API 仅在不增加复杂度时提供，内部以子进程异步等待实现。

## 5. 请求与响应协议

协议文件位于 `protocol/popu.schema.json`。协议可被 CLI、Python 和未来语言绑定共同使用。

### 5.1 对话框请求

```json
{
  "version": "1.0",
  "mode": "dialog",
  "template": "confirm",
  "position": "cursor_screen_center",
  "window": { "always_on_top": true, "width": 440, "height": 260 },
  "content": {
    "title": "Confirm replacement",
    "message": "Three backup files already exist.",
    "details": [{ "label": "Destination", "value": "D:\\Backup" }]
  },
  "form": {
    "fields": [
      { "id": "confirmed", "type": "boolean", "label": "Replace files", "required": true }
    ]
  },
  "actions": [
    { "id": "confirm", "label": "Replace", "style": "danger", "submit": { "confirmed": true } },
    { "id": "cancel", "label": "Cancel", "style": "secondary", "close": true }
  ],
  "timeout_seconds": 300
}
```

### 5.2 通知请求

```json
{
  "version": "1.0",
  "mode": "notify",
  "template": "notification",
  "position": "bottom_right",
  "duration_ms": 8000,
  "content": {
    "severity": "success",
    "title": "Backup completed",
    "message": "Details: 128 files, 2.4 GB, 03:21."
  }
}
```

### 5.3 响应

```json
{
  "version": "1.0",
  "status": "submitted",
  "action": "confirm",
  "values": { "confirmed": true }
}
```

`status` 只能是 `submitted`、`cancelled`、`closed` 或 `timed_out`。`notify` 不产生业务响应；调用方仅接收 `shown` 或错误。

## 6. 位置与窗口策略

v1 只保证以下两种位置，其余位置留给后续版本：

| `position` | 行为 |
| --- | --- |
| `bottom_right` | 在当前活动屏幕工作区的右下角显示，保留系统任务栏与配置的边距。适合通知。 |
| `cursor_screen_center` | 读取当前鼠标坐标，识别鼠标所在屏幕，在该屏幕工作区居中显示。适合需要立即关注的对话框。 |

实现细节：Tauri/Rust 后端负责获取屏幕与鼠标信息、DPI 换算和裁边；HTML 不自行计算屏幕坐标。窗口默认 `always_on_top: true`，但仅第一次出现时尝试获取键盘焦点；通知不抢焦点。多屏、缩放、任务栏位置变化必须有自动化测试和手工测试。

## 7. 模板系统

模板是 Popu 的视觉和布局扩展点，不是给 AI 执行代码的入口。

### 7.1 模板来源

1. **内置模板**：随 `popu.exe` 打包，例如 `confirm`、`form`、`notification`。
2. **本地模板目录**：调用方通过 `--template-dir` 或请求中的受控配置启用；目录由集成方安装和管理，不能由 AI 返回的请求指定任意路径。

每个模板目录包含：

```text
my-template/
├── manifest.json       # 名称、版本、支持的 mode、允许的字段
├── index.html
├── template.css
└── template.js
```

`manifest.json` 声明模板名称和支持的 schema 版本。Host 在显示前校验模板、请求模式和字段；不兼容即失败，不回退到未知模板。

### 7.2 模板桥接 API

模板前端只获得两件事：只读 `request` 数据和窄桥接对象 `popu`。

```javascript
const request = await popu.getRequest();       // read-only structured data
await popu.submit({ action: "confirm", values: { confirmed: true } });
await popu.close({ status: "cancelled" });
```

不开放 Shell、文件系统、网络、Tauri 任意 command、动态导航、`eval` 或远程脚本加载。HTML 中展示文本必须使用安全文本节点或经过受限 sanitizer 的 Markdown；不得将 `content` 字段插入 `innerHTML`。

第三方模板在 v1 被视为本地可信代码。将来若开放模板市场或远程下载，必须增加签名、沙箱和权限清单，不能沿用当前信任模型。

## 8. 进程与内部架构

```text
┌──────────────────────── popu.exe ────────────────────────┐
│ CLI parser                                                 │
│  ├─ dialog：读取 JSON → 校验 → 启动 Tauri window → stdout │
│  └─ notify：启动 detached UI child → 等待 ready → 返回     │
├───────────────────────────────────────────────────────────┤
│ Rust core: schema validation · template resolution         │
│            placement · timeout · response validation       │
├───────────────────────────────────────────────────────────┤
│ Tauri WebView2: packaged / trusted template renderer       │
└───────────────────────────────────────────────────────────┘
```

不需要 Host daemon、SQLite、Named Pipe 或 HTTP server。`dialog` 结果通过该进程的 stdout 返回；`notify` 的 UI 子进程用一次性受控启动参数/匿名管道接收请求并回传 ready 状态，随后不保留任何业务数据。

## 9. Windows 与 Tauri

v1 目标为 Windows 10/11 x64。Tauri 负责原生无边框窗口、always-on-top 和 Windows WebView2；所有前端资源本地打包，默认不加载外部 URL。WebView2 Runtime 使用 Evergreen 分发，安装器应检测运行时是否可用并给出修复方式。固定版 Runtime 会显著增大包体，不作为原型默认方案。 [WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)

未来迁移到 macOS/Linux 时，核心协议、模板清单和 Python/CLI 合约不变；仅窗口定位、安装器和 WebView 运行时适配需要重新验证。

## 10. 安全与失败语义

| 风险 | v1 控制措施 |
| --- | --- |
| AI 注入脚本/HTML | 请求只传 schema 数据；模板是本地可信资源；禁止 `innerHTML` 与远程资源。 |
| 用户误判操作 | 对话框明确展示标题、来源、目标、风险和取消按钮；高风险动作由调用方最终执行。 |
| 用户关闭窗口 | 返回 `closed`；调用方默认不执行副作用。 |
| 用户无响应 | `dialog` 按 `timeout_seconds` 返回 `timed_out`；默认不批准。 |
| 通知失败 | `notify` 返回非零；调用方按自身日志/降级策略处理。 |
| 模板不可信 | v1 仅加载安装方显式配置的本地目录；不支持由请求决定路径。 |

Popu 的“确认”不是操作授权的替代品。对删除、支付、上传、执行命令等动作，调用方应把确定的操作摘要传给 Popu，并在收到有效响应后自行执行。

## 11. 仓库布局

```text
popu/
├── README.md
├── ARCHITECTURE.md
├── LICENSE
├── SECURITY.md
├── protocol/
│   ├── popu.schema.json
│   └── examples/
├── src-tauri/                 # Rust: CLI、校验、窗口、模板、位置策略
├── ui/                        # 内置模板和共享样式
│   └── templates/
│       ├── confirm/
│       ├── form/
│       └── notification/
├── python/                    # Python SDK
├── tests/
│   ├── protocol/
│   ├── templates/
│   └── windows-e2e/
└── packaging/windows/
```

## 12. 原型里程碑

1. **CLI 核心**：实现 schema、`dialog --wait`、stdout 响应及 `confirm` 内置模板。
2. **通知与定位**：实现 `notify`、`bottom_right`、`cursor_screen_center`、多屏 DPI 测试。
3. **可替换模板**：实现 manifest、`--template-dir`、模板验证及桥接 API。
4. **Python SDK 与发布**：提供同步 API、Windows 安装包、WebView2 Runtime 检测、示例与端到端测试。
5. **再评估**：基于 Windows 原型的真实使用，再决定存储/恢复、其他平台、远程通道或其他语言 SDK 是否值得增加。

## 13. FoneTool 的当前关系

Popu 在 `com/popu` 中作为独立仓库原型，不接入 FoneTool C++ 构建系统，也不引入 C++ SDK。现阶段由 Python 自动化或 Shell 调用 `popu.exe` 验证产品体验；若验证成功，再决定是否为 FoneTool 添加专门的调用层。
