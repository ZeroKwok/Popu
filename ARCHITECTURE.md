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
- 不启动常驻守护进程、不使用 SQLite、不开放网络端口；仅允许活动期间的本地进程间协调通道。
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

CLI 启动一个短生命周期 UI 子进程；父进程只等待“窗口创建成功/失败”的握手。通过 Python SDK 调用时，UI 子进程持有调用方提供的 lifetime pipe，并在调用方退出时立即关闭自身。纯 CLI 调用默认只保证窗口自行到期或被关闭；若需要严格生命周期绑定，调用方必须提供 lifetime pipe。通知不落盘，机器重启或 Popu 被关闭后自然消失。

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

### 7.3 v1 渲染控件白名单

所有内置模板和可替换模板都应基于下面的稳定控件契约渲染。AI/调用方可以组合这些控件，但不能请求任意 DOM、脚本或自定义控件类型。

| 分类 | 控件 | 用途 |
| --- | --- | --- |
| 结构 | `title`、`message`、`details` | 标题、纯文本/受限 Markdown 正文、键值详情列表。 |
| 状态 | `severity`、`icon` | `info`、`success`、`warning`、`error` 四种语义状态及对应内置图标。 |
| 输入 | `text`、`textarea` | 短文本与多行反馈；支持必填、长度、placeholder 与正则校验。 |
| 输入 | `checkbox` | 单项确认或“应用到后续项”。 |
| 输入 | `select`、`radio_group` | 有限候选项选择；选项只包含稳定 value 与显示 label。 |
| 输入 | `number` | 数字输入；支持最小值、最大值和步进。 |
| 操作 | `button` | `primary`、`secondary`、`danger` 三种样式，可提交固定值或取消/关闭。 |
| 通知 | `progress`（只读） | 仅展示 0–100 的当前进度；v1 不支持用户拖动或任务控制。 |
| 通知 | `close` | 关闭通知；不会向调用方回传业务结果。 |

内置模板的最小组合为：

- `confirm`：`title + message + details + checkbox? + buttons`
- `form`：`title + message + text/textarea/checkbox/select/radio_group/number + buttons`
- `notification`：`severity + icon + title + message + details? + progress? + close`

v1 暂不支持日期选择器、文件选择器、富文本编辑器、表格编辑、树控件、代码编辑器、图片上传、远程图片、超链接跳转和内嵌网页。确有需求时，应先新增正式 schema 控件类型和跨模板测试，而不是由模板私自解释未知字段。

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

不需要常驻 Host daemon、SQLite 或 HTTP server。Rust 端的临时 coordinator 在有活动请求时使用 Windows Named Pipe 接收本地 CLI 请求，并在最后一个请求结束后立即退出。`dialog` 的 CLI 进程从 coordinator 转发响应至 stdout；`notify` 的 CLI 进程在收到 `ready` 后返回，随后不保留任何业务数据。

### 8.1 调用方生命周期绑定（Windows）

“调用方退出，Popu 也退出”需要调用方协作，不能仅靠 `parent_pid`：PID 可以复用；更重要的是 `popu notify` 为了立即返回会派生 UI 子进程，此时 UI 的直接父进程很快退出。v1 使用 **lifetime pipe** 作为唯一可靠的通用机制。

1. Python SDK 创建匿名管道，将写端保留在 Python 进程，将只读端以受控继承方式传给 Popu（`--lifetime-handle`）。
2. Popu 在 UI 生命周期中异步等待该读端：读端收到 EOF 代表调用方已经关闭管道或进程已退出；此时 Popu 关闭所有窗口并退出，不生成成功响应。
3. `dialog` 通常无需额外 pipe，因为 Python 会等待其子进程；但 SDK 仍传递 pipe，使调用方被强制终止时也能关闭窗口。
4. `notify` 创建窗口并完成 ready 握手后可立即返回给 Python；Python 继续持有写端，因此 UI 能存活到时限、被用户关闭或 Python 退出。

Python 进程崩溃、被结束或正常退出时，操作系统会关闭其管道写端，Popu 将可靠地得到 EOF。该方案也能自然迁移到 Unix。纯 CLI 的 `popu notify` 没有可由其自动继承的“祖先进程存活句柄”，因此默认不承诺绑定；需要严格绑定时由自动化调用方使用 Python SDK，或未来显式传入受控的 `--lifetime-handle`。

Windows Job Object 可以作为原生调用方的替代实现：调用方创建带 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的 Job 并将 Popu 加入其中。它同样要求调用方参与管理，不能由 Popu 单方面建立。

## 9. 并发模型

每个调用是独立会话，拥有独立的 request、窗口、timeout 与 lifetime pipe。因此任意一个调用方退出，只会关闭它自己创建的 Popu 窗口，不影响其他调用方。

```text
Python worker A ── pipe A ──► Popu dialog A
Python worker B ── pipe B ──► Popu notify B
Python worker C ── pipe C ──► Popu dialog C
```

### 9.1 临时 Rust coordinator（v1）

协调逻辑只能存在于 Rust/Popu Host：它是所有 CLI、Python SDK 与未来语言调用进入 UI 前的唯一权威。Python SDK 只负责启动 CLI/维持 lifetime pipe/解析 JSON，不缓存、排队、去重或决定窗口位置。

- 第一个 `popu` 调用取得单例锁并启动 coordinator；后续 CLI 通过当前用户可访问的 Windows Named Pipe 发送请求。
- coordinator 的状态只存在内存：活动请求、请求订阅者、窗口队列、通知堆叠位置及 lifetime pipe。
- `dialog` 按 `concurrency_key` 做 FIFO 互斥；无 key 的 dialog 可同时显示，但 coordinator 负责避免完全重叠。
- `notify` 统一管理右下角的堆叠、动画与到期移除。
- `cursor_screen_center` 仍适合低并发确认；coordinator 在同一屏幕已有中心对话框时按队列展示，避免覆盖。
- 最后一个请求终态完成、所有客户端断开并且没有窗口后，coordinator 关闭 Named Pipe 并退出。

这不是常驻守护进程或任务服务：它不落盘、不接受网络请求，也不能在进程重启后恢复请求。

调用方可通过 `concurrency_key` 表达业务互斥需求，例如同一部署、同一设备或同一订单不应同时出现多个确认框。

### 9.1.1 重复请求与活动期幂等

每个请求应提供调用方生成的 `request_id`，并可选提供 `dedupe_key`。两者语义不同：

- `request_id`：一次业务交互的唯一标识；同一个调用重试时必须保持不变。
- `dedupe_key`：调用方定义的业务去重范围，例如 `"deploy:prod:2026-08-06"`。未提供时默认使用 `request_id`。

Popu **只在请求仍处于活动状态时**使用 key 去重；它不保存终态，因此不能提供跨重启的“永不重复显示”。默认策略如下：

| 场景 | Prototype 行为 |
| --- | --- |
| 任意 CLI/Python 进程中重复 `dialog` | coordinator 合并为同一个窗口；所有等待者收到同一个响应。 |
| 任意 CLI/Python 进程中重复 `notify` | coordinator 更新已显示的通知内容并重新开始显示计时。 |
| 窗口已关闭、提交或超时后再次调用 | 视为新请求，再次显示。长期幂等由调用方保存业务状态。 |

请求可通过 `dedupe_policy` 覆盖默认行为：`coalesce`（合并）、`replace`（更新活动通知）、`allow`（始终新建）。`dialog` 不允许 `replace`，避免用户在填写时请求内容被悄然改变。

## 10. Windows 与 Tauri

v1 目标为 Windows 10/11 x64。Tauri 负责原生无边框窗口、always-on-top 和 Windows WebView2；所有前端资源本地打包，默认不加载外部 URL。WebView2 Runtime 使用 Evergreen 分发，安装器应检测运行时是否可用并给出修复方式。固定版 Runtime 会显著增大包体，不作为原型默认方案。 [WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)

未来迁移到 macOS/Linux 时，核心协议、模板清单和 Python/CLI 合约不变；仅窗口定位、安装器和 WebView 运行时适配需要重新验证。

## 11. 安全与失败语义

| 风险 | v1 控制措施 |
| --- | --- |
| AI 注入脚本/HTML | 请求只传 schema 数据；模板是本地可信资源；禁止 `innerHTML` 与远程资源。 |
| 用户误判操作 | 对话框明确展示标题、来源、目标、风险和取消按钮；高风险动作由调用方最终执行。 |
| 用户关闭窗口 | 返回 `closed`；调用方默认不执行副作用。 |
| 用户无响应 | `dialog` 按 `timeout_seconds` 返回 `timed_out`；默认不批准。 |
| 通知失败 | `notify` 返回非零；调用方按自身日志/降级策略处理。 |
| 模板不可信 | v1 仅加载安装方显式配置的本地目录；不支持由请求决定路径。 |

Popu 的“确认”不是操作授权的替代品。对删除、支付、上传、执行命令等动作，调用方应把确定的操作摘要传给 Popu，并在收到有效响应后自行执行。

## 12. 仓库布局

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

## 13. 原型里程碑

1. **CLI 核心**：实现 Rust coordinator、单例锁、当前用户 Named Pipe、schema、`dialog --wait`、stdout 响应及 `confirm` 内置模板。
2. **通知与定位**：实现 `notify`、`bottom_right`、`cursor_screen_center`、活动期去重、并发队列和多屏 DPI 测试。
3. **可替换模板**：实现 manifest、`--template-dir`、模板验证及桥接 API。
4. **Python SDK 与发布**：提供同步 API、lifetime pipe、Windows 安装包、WebView2 Runtime 检测、示例与端到端测试。
5. **再评估**：基于 Windows 原型的真实使用，再决定存储/恢复、其他平台、远程通道或其他语言 SDK 是否值得增加。
