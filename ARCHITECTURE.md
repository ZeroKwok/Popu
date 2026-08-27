# Popu 架构设计

> 状态：Prototype / Draft 0.2  
> 首发平台：Windows 10/11  
> 建议许可证：Apache-2.0

## 1. 定位

**Popu** 是供自动化、Agent 和脚本调用的本机交互器：需要人决定时弹出一个小窗口，或仅显示一条完成/异常通知。

它不是任务系统、工作流引擎、通知中心或远程审批平台。v1 的边界刻意保持很小：一个调用对应一次 UI 会话；调用结束后不保存任务、不支持稍后恢复，也不连接 Slack、邮件、网页或其他设备。

**原型兼容性原则：** 除非需求明确要求，不维护 CLI、SDK 或协议的向后兼容。接口可直接收敛和重构，调用方与样例必须同步更新。

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
- 不启动 coordinator/常驻守护进程，不使用 SQLite，不开放网络端口或本地 IPC 服务。
- 不支持远程模板 URL、模型下发任意 HTML/JavaScript、iframe 或模板中的任意系统访问。
- 不在 v1 支持 macOS/Linux；设计中保留未来迁移到 Tauri 对应 WebView 的空间。

## 3. 使用模型

### 3.1 对话框：`dialog`

`dialog` 用于确认、选择、表单输入和人工反馈。调用方启动 Popu 并等待，用户提交、取消、关闭窗口或超时后，Popu 通过 stdout 返回一个 JSON 响应并退出。

```text
caller ── spawn popu request request.json ──► user action ──► JSON response ──► caller
```

这是短生命周期同步会话：调用方退出时，Popu 应结束该会话并关闭窗口；Popu 崩溃则调用方得到运行错误。业务端必须把 `cancelled`、`closed` 和 `timed_out` 当作正常且安全的结果处理。

### 3.2 通知：`notify`

`notify` 用于“任务执行完成”“操作失败”“发现可处理事项”等只需传达、不等待决定的场景。

```text
caller ── popu request notification.json ──► Popu 确认窗口已创建 ──► caller 立即继续
                              │
                              └── 用户关闭 / duration 到期后退出
```

CLI 启动一个短生命周期 UI 子进程；父进程只等待“窗口创建成功/失败”的握手。通过 Python SDK 调用时，UI 子进程持有调用方提供的 lifetime pipe，并在调用方退出时立即关闭自身。纯 CLI 调用默认只保证窗口自行到期或被关闭；若需要严格生命周期绑定，调用方必须提供 lifetime pipe。通知不落盘，机器重启或 Popu 被关闭后自然消失。

## 4. CLI 与 Python API

### 4.1 CLI

```text
popu request request.json
popu request notification.json --template .\templates\my-template
popu templates list
popu templates validate --path .\templates\my-template
```

- `request`：读取 JSON 后由 `mode` 决定行为。`dialog` 的 stdout 仅输出一个 `InteractionResponse` JSON；`notify` 在窗口创建后输出 `{ "status": "shown" }` 并立即返回。`shown` 仅表示窗口已创建，不表示通知在后续展示期间不会崩溃、被系统关闭或被用户关闭。
- 退出码：`0` 请求已正常完成/显示；`10` 用户取消或关闭；`11` 对话框超时；`12` 请求、模板或运行时错误。

这里的“返回”指通知 `popu request` 父进程退出。Windows PowerShell 5.1 的 native pipeline 可能继续跟踪仍在显示的 UI 后代；要求提示符也立即恢复时应使用 `Start-Process` 或 Python SDK。

### 4.2 Python SDK

Python 包名建议为 `popu`，仅负责构造和校验 JSON、调用已安装的 `popu.exe`、解析返回值。它不嵌入 Tauri，也不自行实现窗口。

```python
from popu import DialogRequest, Notification, Position, show, notify

result = show(DialogRequest.confirm(
    title="Replace files?",
    message="3 existing files will be replaced.",
    position=Position.CURSOR_SCREEN_CENTER,
))

if result.status == "submitted" and result.action == "confirm":
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
      { "id": "confirmed", "type": "checkbox", "label": "Replace files", "required": true }
    ]
  },
  "actions": [
    { "id": "confirm", "label": "Replace", "style": "danger", "submit": true },
    { "id": "cancel", "label": "Cancel", "style": "secondary", "result_status": "cancelled" }
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

### 5.4 表单与 action 语义

- `form.fields[].type` 使用渲染控件名称：`text`、`textarea`、`checkbox`、`select`、`radio_group`、`number`。
- 用户输入是 `values` 的唯一来源；action 不得注入、覆盖或隐式修改任何字段。
- 模板提交的 `values` 不得包含未知字段，且必须满足必填与类型约束；Rust 端重新校验。
- `submit: true` 表示先校验所有字段，再返回 `status: "submitted"`、当前 `values` 与该 action ID。
- `result_status: "cancelled"` 表示立即返回取消；标题栏关闭和 Alt+F4 返回 `closed`；超时返回 `timed_out`。
- v1 不支持 action 携带固定业务字段。需要固定业务选择时，使用 `select` 或 `radio_group` 字段显式呈现给用户。

## 6. 位置与窗口策略

v1 只保证以下两种位置，其余位置留给后续版本：

| `position` | 行为 |
| --- | --- |
| `bottom_right` | 在**请求创建瞬间鼠标所在屏幕**的工作区右下角显示，保留系统任务栏与配置的边距。读取鼠标失败时使用主屏。适合通知。 |
| `cursor_screen_center` | 读取当前鼠标坐标，识别鼠标所在屏幕，在该屏幕工作区居中显示。适合需要立即关注的对话框。 |

实现细节：Tauri/Rust 后端负责获取屏幕与鼠标信息、DPI 换算和裁边；HTML 不自行计算屏幕坐标。窗口默认 `always_on_top: true`，但仅第一次出现时尝试获取键盘焦点；通知不抢焦点。多屏、缩放、任务栏位置变化必须有自动化测试和手工测试。

## 7. 模板系统

模板是 Popu 的视觉和布局扩展点，不是给 AI 执行代码的入口。

### 7.1 模板来源

1. **内置模板**：随 `popu.exe` 打包，例如 `confirm`、`form`、`notification`。
2. **本地模板目录**：启动方通过 `--template` 的受信任配置启用；目录由集成方安装和管理，不能由 AI 返回的请求指定任意路径。该参数不是请求 JSON 的一部分。

每个模板目录包含：

```text
my-template/
├── manifest.json       # 名称、版本、支持的 mode、允许的字段
├── template.html
└── template.css
```

模板可使用以下 Popu 交互标记：`data-popu-drag-region` 定义可拖动窗口的背景区域，`data-popu-selectable` 定义允许用户选中和复制的文本区域。内置模板将卡片背景标为 drag region，正文和详情标为 selectable；按钮、表单控件、标签和 selectable 元素本身不会触发拖动。

`manifest.json` 声明模板名称和支持的协议版本。当前 Rust 进程在显示前校验模板名称、请求模式、必需 slot 和路径边界；不兼容即失败，不回退到未知模板。v1 外部模板仅替换 HTML/CSS 布局，不加载模板自带 JavaScript。

### 7.2 模板桥接 API

打包的通用渲染器只获得只读 `request` 数据和以下窄桥接能力：

```javascript
const bootstrap = await invoke("get_bootstrap");
await invoke("submit", { action: "confirm", values: collectFormValues() });
await invoke("finish", { status: "cancelled", action: "cancel" });
```

模板 HTML 本身来自受信任的安装内容，可作为布局载入；所有请求中的 `title/message/details/label/value` 只能通过 `textContent` 或表单属性写入，不得拼接进模板 HTML。应用强制使用 CSP，禁止远程脚本、远程图片和网络请求；同时不开放 Shell、文件系统、任意 Tauri command 或 `eval`。

第三方本地模板在 v1 被视为安装方的可信布局，但仍不执行模板 JavaScript。模板只能改变结构与样式，交互、校验和 IPC 均由随 Popu 打包的通用渲染器完成。

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
| 操作 | `button` | `primary`、`secondary`、`danger` 三种样式，可提交当前表单或取消。 |
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
│  ├─ request：读取 JSON → 校验 → 按 mode 分派              │
│  ├─ dialog mode：启动 Tauri window → stdout               │
│  └─ notify mode：启动 detached UI child → 等待 ready → 返回│
├───────────────────────────────────────────────────────────┤
│ Rust core: schema validation · template resolution         │
│            placement · timeout · response validation       │
├───────────────────────────────────────────────────────────┤
│ Tauri WebView2: packaged / trusted template renderer       │
└───────────────────────────────────────────────────────────┘
```

不需要 Host daemon、SQLite、Named Pipe 或 HTTP server。`request` 的 dialog mode 进程自身拥有 Tauri 窗口并通过 stdout 返回响应；notify mode 的 CLI 进程启动一个短生命周期 UI 子进程，在收到 `ready` 后返回。该 UI 子进程继承请求数据和可选 lifetime pipe，随后不保留任何业务数据。

### 8.1 调用方生命周期绑定（Windows）

“调用方退出，Popu 也退出”需要调用方协作，不能仅靠 `parent_pid`：PID 可以复用；更重要的是 notify mode 为了立即返回会派生 UI 子进程，此时 UI 的直接父进程很快退出。v1 使用 **lifetime pipe** 作为唯一可靠的通用机制。

1. Python SDK 创建匿名管道，将写端保留在 Python 进程，将只读端以受控继承方式传给 Popu（`--lifetime-handle`）。
2. Popu 在 UI 生命周期中异步等待该读端：读端收到 EOF 代表调用方已经关闭管道或进程已退出；此时 Popu 关闭所有窗口并退出，不生成成功响应。
3. `dialog` 通常无需额外 pipe，因为 Python 会等待其子进程；但 SDK 仍传递 pipe，使调用方被强制终止时也能关闭窗口。
4. `notify` 创建窗口并完成 ready 握手后可立即返回给 Python；Python 继续持有写端，因此 UI 能存活到时限、被用户关闭或 Python 退出。

Python 进程崩溃、被结束或正常退出时，操作系统会关闭其管道写端，Popu 将可靠地得到 EOF。该方案也能自然迁移到 Unix。纯 CLI 的 notify request 没有可由其自动继承的“祖先进程存活句柄”，因此默认不承诺绑定；需要严格绑定时由自动化调用方使用 Python SDK，或未来显式传入受控的 `--lifetime-handle`。

**句柄所有权规则：** Python 仅保留 pipe 写端；CLI 和 `notify` UI 子进程仅继承读端。CLI 在派生 UI 后必须关闭自己的读端副本；UI 绝不能继承任何写端。否则即使 Python 退出，仍有写端存活，UI 也不会收到 EOF。

Windows Job Object 可以作为原生调用方的替代实现：调用方创建带 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的 Job 并将 Popu 加入其中。它同样要求调用方参与管理，不能由 Popu 单方面建立。

## 9. 并发模型

每个调用是独立会话，拥有独立的 request、窗口、timeout 与 lifetime pipe。因此任意一个调用方退出，只会关闭它自己创建的 Popu 窗口，不影响其他调用方。

```text
Python worker A ── pipe A ──► Popu dialog A
Python worker B ── pipe B ──► Popu notify B
Python worker C ── pipe C ──► Popu dialog C
```

### 9.1 CLI 原型的边界


- 每次 `dialog` 均显示独立窗口；不对不同调用做队列、互斥或去重。
- 每次 `notify` 均显示独立窗口；同一屏幕的右下角通知可能重叠，原型阶段接受该限制。
- `cursor_screen_center` 的并发窗口也可能重叠，因此只适用于低并发、需要立即处理的确认。
- 同一个请求多次调用等同于多个请求；业务层若需要幂等、合并、取消或“仅提示一次”，必须在调用 Popu 前完成判断。

这条边界使 CLI 保持真正的“一次调用、一扇窗口、一次响应”模型，不产生隐式跨进程状态。

### 9.2 未来升级：IPC Host 的活动期幂等

只有在需要跨进程去重、全局通知堆叠、对话框队列、`concurrency_key` 互斥或跨调用取消时，才引入下面的**替代运行模型**；不能在当前 CLI 中暗藏这些状态：

```text
CLI / Python SDK ── IPC ──► Popu Host（唯一 UI owner）
                                  │
                                  └── in-memory active-session state
```

每个请求应提供调用方生成的 `request_id`，并可选提供 `dedupe_key`。两者语义不同：

- `request_id`：一次业务交互的唯一标识；同一个调用重试时必须保持不变。
- `dedupe_key`：调用方定义的业务去重范围，例如 `"deploy:prod:2026-08-06"`。未提供时默认使用 `request_id`。

以下能力不属于当前 CLI 原型；仅在明确升级为 IPC Host 后由 Host 实现。Host 只在请求仍处于活动状态时使用 key 去重；它不保存终态，因此不能提供跨重启的“永不重复显示”。默认策略如下：

| 场景 | 未来 IPC Host 行为 |
| --- | --- |
| 任意 CLI/Python 进程中重复 `dialog` | Host 合并为同一个窗口；所有等待者收到同一个响应。 |
| 任意 CLI/Python 进程中重复 `notify` | Host 更新已显示的通知内容并重新开始显示计时。 |
| 窗口已关闭、提交或超时后再次调用 | 视为新请求，再次显示。长期幂等由调用方保存业务状态。 |

请求可通过 `dedupe_policy` 覆盖默认行为：`coalesce`（合并）、`replace`（更新活动通知）、`allow`（始终新建）。`dialog` 不允许 `replace`，避免用户在填写时请求内容被悄然改变。

## 10. Windows 与 Tauri

v1 目标为 Windows 10/11 x64。Tauri 负责原生无边框窗口、always-on-top 和 Windows WebView2；所有前端资源本地打包，默认不加载外部 URL。窗口和 WebView 使用透明背景，Windows 通过原生圆角 Window Region 让圆角外部的透明区域鼠标穿透到底层窗口；其他平台以后按各自窗口系统能力适配。WebView2 Runtime 使用 Evergreen 分发，安装器应检测运行时是否可用并给出修复方式。固定版 Runtime 会显著增大包体，不作为原型默认方案。 [WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)

未来迁移到 macOS/Linux 时，核心协议、模板清单和 Python/CLI 合约不变；仅窗口定位、安装器和 WebView 运行时适配需要重新验证。

## 11. 安全与失败语义

| 风险 | v1 控制措施 |
| --- | --- |
| AI 注入脚本/HTML | 请求只传 schema 数据；可信模板 HTML 与请求数据分离；请求文本只写入安全文本节点；禁止远程资源。 |
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
└── src-tauri/tests/         # CLI 与协议 fixture 集成测试
```

## 13. 测试策略与用例

测试以“Rust 核心逻辑可确定地单测、真实 WebView 仅做少量端到端验证”为原则。窗口位置、调用隔离和生命周期不能只靠人工点一点确认。

### 13.1 协议与 CLI（自动化）

| 用例 | 断言 |
| --- | --- |
| 最小 `dialog` 请求 | 校验成功，补齐默认位置、窗口大小和模板。 |
| 最小 `notify` 请求 | 校验成功，`duration_ms` 使用默认值。 |
| 缺少 `version`、未知 mode/字段类型 | 拒绝请求，退出码 `12`，不得创建窗口。 |
| required `checkbox` 未勾选后提交 | 拒绝提交；不得由 action 写入 `values` 绕过校验。 |
| Cancel action、标题栏关闭、超时 | 分别返回 `cancelled`、`closed`、`timed_out`。 |
| `notify` 包含 form 或等待响应 action | 拒绝请求。 |
| 关闭/取消/超时 | stdout 分别只输出 `closed`、`cancelled`、`timed_out` JSON，退出码符合 CLI 合约。 |
| stdout/stderr 分离 | stdout 可被 `json.loads()` 直接解析；诊断与日志绝不污染 stdout。 |
| 同一 schema fixture 经 CLI 和 Python SDK | 请求与响应 JSON 完全一致。 |

### 13.2 模板与渲染安全（自动化）

| 用例 | 断言 |
| --- | --- |
| 内置 `confirm/form/notification` | 每种白名单控件都能正确渲染、提交和校验。 |
| 合法外部模板 manifest | 模式、schema 版本、允许字段匹配时才可加载。 |
| 缺 manifest、未知模板、版本不匹配 | 明确失败，不静默回退。 |
| `title/message/details` 中包含 `<script>`、`<img onerror>`、`javascript:` | 仅作为文本显示；不执行脚本、不发起网络请求。 |
| 模板请求未知 bridge command | Rust 端拒绝，前端不能取得文件系统/Shell/网络权限。 |
| 内置模板尝试 `fetch`、跳转或加载远程图片 | CSP/导航拦截拒绝请求。 |
| 模板目录路径穿越 | `..`、符号链接越界和请求中动态指定模板路径均被拒绝。 |
| 字段校验 | 必填、长度、正则、数字范围错误时禁用提交并显示固定错误提示。 |

### 13.3 独立调用与并发（Rust 单元/集成测试）

| 用例 | 断言 |
| --- | --- |
| 两个相同 dialog 请求 | 创建两个独立窗口；响应不能串到对方 stdout。 |
| 两个相同 notify 请求 | 创建两个独立通知；无共享状态。 |
| 两个并发调用方 | 结束 A，只关闭 A 的窗口；B 继续可交互。 |
| 多个右下角通知 | 每个窗口按自身工作区右下角计算；跨进程重叠属于已知原型限制。 |
| 中心对话框同屏并发 | 可能重叠；这是已知原型限制。 |

### 13.4 lifetime pipe 与异常退出（Windows 集成测试）

测试工具启动一个可控的假调用方进程，记录 Popu 子进程 PID 和窗口标识，再触发对应故障。每个用例都必须验证：窗口关闭、Popu 退出、没有 stdout 成功响应、没有孤儿进程。

| 用例 | 操作 |
| --- | --- |
| 正常调用方退出 | 关闭 lifetime pipe 写端。 |
| 调用方崩溃 | `TerminateProcess` 假调用方。 |
| dialog 等待中退出 | 用户尚未点击时结束调用方。 |
| notify 显示中退出 | 通知尚未到期时结束调用方。 |
| 两个调用方并发 | 结束 A，只关闭 A 的窗口；B 继续可交互。 |
| pipe 创建/传递失败 | 请求失败且不创建不可关闭的 UI。 |
| Python 已退出但 CLI/UI 遗留写端副本 | 测试失败；正确实现必须使 UI 收到 EOF 并退出。 |

### 13.5 位置、DPI 与可访问性

位置计算必须抽成无 UI 的纯 Rust 函数，以虚拟显示器参数做单元测试：单屏、双屏横向/纵向、负坐标、125%/150%/200% DPI、任务栏在四个边、鼠标刚好位于屏幕边界、窗口大于工作区。

Windows 端到端测试至少覆盖：右下角窗口不遮挡任务栏；鼠标所在屏幕中心定位正确；Tab 顺序、Enter 默认动作、Esc 取消、可见焦点与屏幕阅读器名称；亮/暗主题、最小与高 DPI；以及 WebView2 缺失时显示可诊断错误而不是崩溃。

透明窗口测试还必须覆盖：圆角外部点击穿透至底层测试窗口、卡片内部可正常点击；WebView 四边保留固定 9px 透明阴影区，卡片和阴影均不得溢出该区域；拖动 `data-popu-drag-region` 的空白区域会移动窗口；正文/详情可选择复制但不触发拖动；标题、背景和按钮文字不可选择。`dialog-short-content.json` 验证紧凑短内容，`dialog-long-content.json` 验证换行、详情、表单滚动和操作区可达性，`dialog-ultra-long-content.json` 验证 2000px 高度上限下的内容溢出和统一滚动策略。

多显示器与真实 DPI 组合很难在普通 CI 完整模拟：CI 运行纯位置函数和单屏 UI smoke test，发布前在物理双屏 Windows 机器执行手工验收矩阵。

### 13.6 测试夹具与质量门槛

- `protocol/examples/` 中每个合法请求都是跨 CLI、Python 与 Rust 的共享 fixture；非法请求放入 `invalid/`。
- Rust 单元测试覆盖 schema、响应校验、位置和 pipe 生命周期；不依赖真实 WebView。
- UI 使用稳定的 `data-testid` 与可访问性名称，避免以像素或任意 CSS class 作为自动化选择器。
- CI 至少运行格式化、lint、Rust 单元/集成测试、Python 测试、schema fixture 校验和 Windows UI smoke test。
- 对 lifetime pipe、模板桥接和窗口定位这三类高风险逻辑要求回归测试；修复 bug 时先添加能复现问题的测试。

## 14. 原型里程碑

1. **CLI 核心**：实现 schema、`request <REQUEST>`、stdout 响应及 `confirm` 内置模板。
2. **通知与定位**：实现 notify mode、`bottom_right`、`cursor_screen_center`、多窗口独立生命周期和多屏 DPI 测试。
3. **可替换模板**：实现 manifest、`--template`、模板验证及桥接 API。
4. **Python SDK 与发布**：提供同步 API、lifetime pipe、Windows 安装包、WebView2 Runtime 检测、示例与端到端测试。
5. **再评估**：基于 Windows 原型的真实使用，再决定存储/恢复、其他平台、远程通道或其他语言 SDK 是否值得增加。
