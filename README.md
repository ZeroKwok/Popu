# Popu

> 当前架构：单实例常驻 Tauri Host。CLI 与 Python SDK 通过 `interprocess::local_socket`（Windows Named Pipe / Unix Domain Socket）提交和管理请求；Host 统一拥有 UUID、窗口、FIFO 队列、revision 与终态。

Host 在首次请求时自动启动。notification 被接受后独立存活；dialog 等待终态。管理命令包括 `popu update <id> <update.json>`、`dismiss`、`status`、`wait`、`host status` 和 `host stop`。请求可设置 `coalesce_key`，更新文件必须携带最新 `expected_revision` 和完整 `content`。

Popu 是一个面向自动化脚本和 AI Agent 的轻量本机交互窗口。任务需要人工决定时显示对话框；只需告知结果时显示短暂通知。

当前版本使用 Tauri 2 + WebView2 的单实例常驻 Host。Rust CLI 与 Python SDK 通过 `interprocess::local_socket` 连接 Host；Windows 使用 Named Pipe，Unix 使用 Unix Domain Socket。Host 统一拥有窗口、队列、更新和请求终态。

原型阶段默认**不提供向后兼容**：除非需求明确要求，CLI、SDK 和协议接口可以直接重构，调用方应随版本同步更新。

## 功能

- JSON 的 `mode: "dialog"`：等待用户提交、取消、关闭或超时，向 stdout 返回 JSON。
- JSON 的 `mode: "notify"`：窗口创建后返回，通知到期自动退出。
- 内置 `confirm`、`form`、`notification` 三种模板。
- 可信本地 HTML/CSS 模板替换；模板不执行自带 JavaScript。
- 可选鼠标所在屏幕或主屏；尺寸支持逻辑像素与百分比，同一尺寸配置适用于 1080p、2K 和 4K 屏幕；`"100%" × "100%"` 填满所选工作区或物理屏幕。
- 当宽高均为 `"100%"` 时，窗口不允许拖动，并移除圆角与阴影；超长内容仅在中间内容区滚动。
- 无边框透明圆角窗口；Windows 圆角外部区域鼠标穿透到底层窗口。
- `data-popu-drag-region` 定义拖拽背景，`data-popu-selectable` 定义可复制文本；内置模板默认允许复制所有展示内容。
- Host 生成 UUID `request_id`，支持 `coalesce_key`、revision 冲突检测和完整内容快照更新。
- dialog 全局 FIFO 单活跃；notification 在主显示器最多显示三个，溢出后 FIFO 排队。
- notification 可设置 `requires_read: true` 声明需要用户阅读，默认 `false`；dialog 不允许携带该字段。超时或展示失败的通知保留未读状态，只有需要阅读且未读的历史记录会持续触发托盘提醒。

## 通知阅读与历史

通知的请求终态和阅读状态独立：超时仍为 `expired`，阅读历史不会改变终态。主动关闭通知视为已知悉；调用方撤销或“Clear current notifications”也不留下待阅读提醒。

历史窗口左侧以圆点和加粗标题表示未读，需要阅读的未读条目额外显示 “Awaiting reading” 标记。顶部同时显示待阅读数和全部未读数。只有详情在历史窗口前台完成展示后，该条记录才标记为已读；后台刷新和最小化不会批量清除未读。新记录到达时保留当前选择。

托盘左键单击或双击打开／恢复历史窗口，右键显示菜单。托盘仅在存在 `requires_read: true` 且未读的历史记录时持续提醒，正在显示或排队的通知不触发提醒；阅读其中一条不会清除其他待阅读记录的提醒。无待阅读历史时提示文字为 “Popu”，有待阅读历史时显示对应数量。阅读状态会在 Host 重启后保留，历史最多保存最近 500 条。

可以运行 `just test notification-requires-read.json` 和 `just test notification-ordinary-unread.json` 对比两类未读行为。Python 调用使用 `Notification.success(title="Review result", requires_read=True)`。

既有历史内容会保留，并按新字段默认值处理为普通未读；不恢复旧全局提醒标志的含义，不提供新旧协议或阅读状态的兼容层。

## 组织展示内容

先确定用户完成当前判断或操作必须看到哪些信息，再按阅读顺序组织 `content.blocks`。不要把日志结构原样搬进窗口。

| 要展示的数据 | 选择的 block |
| --- | --- |
| 一段结论、影响或简短说明 | `paragraph` |
| 具有稳定标签的关键事实，如目标、数量、耗时、策略 | `properties` |
| 没有先后关系的并列事项 | `unordered_list` |
| 有执行顺序、优先级或阶段关系的事项 | `ordered_list` |
| 0–100 的完成百分比 | `progress` |
| 图片、截图、调用方生成的 SVG 图形 | `image` |
| 数据驱动的折线图、柱状图、热力图或组合图 | `chart`，使用 Vega-Lite 配置 |
| 需要用户填写或确认的少量结构化值 | `form`；仅用于 dialog |

建议按“结论 → 关键事实 → 影响或步骤 → 当前进度 → 必要输入”的顺序排列。每个 block 都可使用简短 `title` 分组；没有标题时直接省略，不要重复窗口主标题。

`properties.items` 的 `label` 与 `text` 都可省略或留空，但每项至少一侧必须有可见内容；仅有 `label` 时跨两列展示，仅有 `text` 时仍从 text 列左对齐。`form.fields[].label` 是可选展示信息，省略或留空时不会创建标签元素，字段的 `id` 与 `type` 仍是必填关键字段。

`content.vertical_align` 可设为 `center`（默认）或 `start`。居中模式只在正文短于可用区域时生效；内容溢出后仍从顶部开始并正常滚动。

内容较多时应压缩表达，但必须保留会改变判断或操作结果的信息：

- 保留结论、影响对象、范围、数量和单位、下一步，以及必要的错误原因或关联 ID。
- 将大量重复项压缩为“总数 + 范围 + 少量代表项”，例如“Documents 中的 1,248 个文件”，不要逐项列出。
- 路径只有在区分目标或避免误操作时才保留；否则使用有意义的目录名或文件名。
- 省略重复元数据、完整日志、堆栈、实现细节和不影响当前决定的信息。
- 表格、树、长报告、复杂编辑或完整日志应交给调用方产品界面，Popu 只展示摘要和下一步。

## 构建

需要 Rust stable、Node.js 20+、Windows 10/11 和 WebView2 Runtime。Node.js 用于将图表引擎打包为离线资源，运行 Popu 时无需 Node.js 或网络。

```powershell
npm ci
npm run build
```

也可在安装了 Rust 与 `just` 的 Windows、macOS 或 Linux 环境中运行：

```powershell
just test dialog-long-content.json
just test dialog-ultra-long-content.json
```

开发期也可直接生成未打包的可执行文件：

```powershell
npm run build:ui
cargo build --manifest-path src-tauri/Cargo.toml
```

产物位于 `src-tauri/target/debug/popu.exe` 或 `src-tauri/target/release/popu.exe`。

`just build` 和 `just build-release` 会自动执行 `build-ui`；初次使用先运行 `npm ci`。`ui/vendor` 是生成物，不提交到版本库。图表依赖和许可证一起打包到可执行文件中，不从 CDN 加载。

## 图片、自定义图标与通用图表

dialog、notification 和历史详情共用渲染器。`content.icon` 可提供 `{ "src": "data:image/...;base64,...", "alt": "图标说明" }` 替换标题旁的默认图标；severity 仍保留，历史列表仍显示 severity 图标。

图片块使用 `{ "type": "image", "source": { "src": "data:image/svg+xml;base64,...", "alt": "说明" }, "caption": "可选说明", "height": 240 }`。支持 SVG、PNG、JPEG 和 WebP；资源必须内嵌，不能使用本地路径或远程 URL。SVG 作为图片加载，支持静态样式、渐变和本地片段引用，不支持脚本、事件或外部资源。图片自动保持比例并限制在可用宽度内，`height` 是图片显示高度上限。

图表块使用 `{ "type": "chart", "engine": "vega_lite", "spec": { ... }, "height": 220 }`。`spec` 是 Vega-Lite 6 配置，折线、柱状、热力、分层等图形共用同一个协议类型；数据通过 `data.values` 或内嵌 `datasets` 提供。`height` 是图表总显示高度，宽度随容器变化重新排版。图表使用交互 SVG，默认悬停显示编码字段的具体值；通过 `encoding.tooltip` 配置字段、标题和数值格式，设为 `null` 或 `mark.tooltip: false` 可关闭提示。

Vega-Lite 的 `spec.params` 可声明悬停高亮、最近点、图例选择以及绑定坐标轴的缩放和平移。Popu 不自动把所有图表变成可缩放图表；这些行为由配置决定，不需要新增协议字段。图表区域不会触发窗口拖拽，提示框保持在窗口范围内，并适配明暗主题。更新或切换历史条目时会销毁旧图表、事件和提示框。交互状态用于本次辅助阅读，不改变请求结果，也不回写历史数据；通知仍按 `duration_ms` 到期，需要长时间探索时可打开历史。

修改完整 `content` 可以更新数据与图表并保留 dialog 表单输入。配置编译在独立工作线程中进行，5 秒超时；页面中的本地 Vega 运行实例负责 SVG 绘制和交互，使用 CSP 兼容解释器，不开启 `unsafe-eval`，不从网络加载资源。`image` 块仍作为静态图片展示。

默认图片高度上限为 320，默认图表高度为 220，可设置范围为 64–1200。单张图片 data URI 不超过 256 KiB，栅格图片每边最多 4096 像素；图表配置不超过 128 KiB，嵌套不超过 32 层，配置节点不超过 20000；完整 content 不超过 768 KiB。SVG 节点最多 10000。图表配置禁止 `url`、`href` 属性，数据及资源不从网络加载。图片和图表就绪后才开始通知计时；历史详情就绪后才提交已读。

典型示例（行情数据均为模拟）：

| 文件 | 展示内容 |
| --- | --- |
| `notification-visual-line.json` | 自定义图标、模拟贵金属折线图 |
| `notification-visual-heatmap.json` | 模拟板块热力图、待阅读通知 |
| `notification-visual-image.json` | 内嵌 SVG 图片与说明 |
| `dialog-visual-bars.json` | 柱状图与必填确认表单 |
| `dialog-visual-heatmap.json` | 热力图叠加数字标签 |
| `dialog-visual-image.json` | 自定义图标、图片与属性说明 |
| `dialog-visual-interactive.json` | 图例选择、悬停取值、滚轮缩放与拖动平移 |

```powershell
just test notification-visual-line.json
just test dialog-visual-bars.json
just build-test notification-visual
just build-test dialog-visual
```

验证命令为 `npm run check:ui`、`npm run test:ui`、`npm run test:visuals`（使用本机 Microsoft Edge 无头浏览器）以及 `cargo test --manifest-path src-tauri/Cargo.toml`。浏览器测试覆盖明暗主题、高 DPI、离线渲染、表单提交及历史已读行为。

Windows 下构建调试版后，还可运行 `npm run test:native` 验证实际 WebView2 加载。运行前应退出已有 Host；测试会短暂显示通知和自动超时的 dialog，使用 `test-results/native-*` 下的隔离历史库，结束后关闭自己启动的 Host，不清理或改写用户历史。

历史列表只查询标题、severity 和阅读状态等摘要；选中记录后通过 `history_get`（RPC 为 `history.get`）读取完整图表和图片，避免一次加载全部历史图片。

调试 WebView2 时，可在 Debug 构建中设置 `POPU_DEVTOOLS=1` 后运行请求；这会自动打开浏览器开发者工具。该开关在 Release 构建中无效。

## CLI

```powershell
popu request protocol/examples/dialog-confirm.json
popu request protocol/examples/notification-success.json
popu request protocol/examples/notification-custom-template.json --template .\protocol\examples\templates\minimal-notification
popu update <request-id> update.json
popu dismiss <request-id>
popu status <request-id>
popu wait <request-id>
popu host status
popu host stop
popu templates list
popu templates validate --path path/to/my-template
```

布局手工验收可使用 `dialog-unordered-list.json`、`dialog-ordered-list.json`、`dialog-all-elements.json`、`notification-success.json`、`dialog-ultra-long-content.json` 和 `dialog-fullscreen-screen.json`；它们分别验证两种列表、六类内容块与全部表单控件、通知进度、长内容滚动，以及物理屏幕全屏。

可替换模板示例位于 `protocol/examples/templates/minimal-notification`。

Host 在首次请求时自动启动。notification 被 Host 接受后 CLI 立即返回；它不依赖提交进程继续存活。dialog 则保持等待直到终态，并采用终态中的 action 退出码。

默认退出码：`0` 提交、确认成功或通知已显示，`10` 取消/关闭，`11` 超时，`12` 请求、模板或运行时错误。dialog action 可用 `exit_code: 0..255` 覆盖自身默认码；即使与默认码重合，也应结合 stdout response 判断业务结果。stdout 只输出机器可读 JSON，诊断写入 stderr。

## Python

新版 `Client` 提供 `create`、`update`、`dismiss`、`status` 和 `wait`，返回 `request_id`、revision、队列状态及 duplicate 信息，不再使用 lifetime pipe。

Python SDK 已同步 Host 协议。`Client` 提供 `create`、`update`、`dismiss`、`status`、`wait`，并返回 `request_id`、`revision`、队列状态和 `duplicate`。

完整协议与设计边界见 [ARCHITECTURE.md](ARCHITECTURE.md)，JSON Schema 见 [protocol/popu.schema.json](protocol/popu.schema.json)。

## 测试

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
node --check ui/app.js
python -m pytest python/tests -q
```

物理多屏、DPI、键盘可访问性和人工点击流程仍需按架构文档中的 Windows 验收矩阵执行。

## 许可证

Apache License 2.0。参见 [LICENSE](LICENSE)。
