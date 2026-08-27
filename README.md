# Popu

Popu 是一个面向自动化脚本和 AI Agent 的轻量本机交互窗口。任务需要人工决定时显示对话框；只需告知结果时显示短暂通知。

当前版本是 Windows 原型：Tauri 2 + WebView2、Rust CLI、Python SDK。它不运行守护进程，不持久化任务，也不做跨调用协调；每次调用拥有独立进程、窗口和结果。

原型阶段默认**不提供向后兼容**：除非需求明确要求，CLI、SDK 和协议接口可以直接重构，调用方应随版本同步更新。

## 功能

- JSON 的 `mode: "dialog"`：等待用户提交、取消、关闭或超时，向 stdout 返回 JSON。
- JSON 的 `mode: "notify"`：窗口创建后返回，通知到期自动退出。
- 内置 `confirm`、`form`、`notification` 三种模板。
- 可信本地 HTML/CSS 模板替换；模板不执行自带 JavaScript。
- 可选鼠标所在屏幕或主屏；尺寸支持像素与百分比，`"100%" × "100%"` 填满所选工作区或物理屏幕。
- 当宽高均为 `"100%"` 时，窗口不允许拖动，并移除圆角与阴影；超长内容仅在中间内容区滚动。
- 无边框透明圆角窗口；Windows 圆角外部区域鼠标穿透到底层窗口。
- `data-popu-drag-region` 定义拖拽背景，`data-popu-selectable` 定义可复制文本；内置模板默认允许复制所有展示内容。
- Python lifetime pipe：Python 调用方退出时关闭对应 Popu 窗口。

## 组织展示内容

先确定用户完成当前判断或操作必须看到哪些信息，再按阅读顺序组织 `content.blocks`。不要把日志结构原样搬进窗口。

| 要展示的数据 | 选择的 block |
| --- | --- |
| 一段结论、影响或简短说明 | `paragraph` |
| 具有稳定标签的关键事实，如目标、数量、耗时、策略 | `properties` |
| 没有先后关系的并列事项 | `unordered_list` |
| 有执行顺序、优先级或阶段关系的事项 | `ordered_list` |
| 0–100 的完成百分比 | `progress` |
| 需要用户填写或确认的少量结构化值 | `form`；仅用于 dialog |

建议按“结论 → 关键事实 → 影响或步骤 → 当前进度 → 必要输入”的顺序排列。每个 block 都可使用简短 `title` 分组；没有标题时直接省略，不要重复窗口主标题。

`properties.items` 的 `label` 与 `text` 都可省略或留空，但每项至少一侧必须有可见内容；单侧有值时按整行展示。`form.fields[].label` 是可选展示信息，省略或留空时不会创建标签元素，字段的 `id` 与 `type` 仍是必填关键字段。

内容较多时应压缩表达，但必须保留会改变判断或操作结果的信息：

- 保留结论、影响对象、范围、数量和单位、下一步，以及必要的错误原因或关联 ID。
- 将大量重复项压缩为“总数 + 范围 + 少量代表项”，例如“Documents 中的 1,248 个文件”，不要逐项列出。
- 路径只有在区分目标或避免误操作时才保留；否则使用有意义的目录名或文件名。
- 省略重复元数据、完整日志、堆栈、实现细节和不影响当前决定的信息。
- 表格、树、长报告、复杂编辑或完整日志应交给调用方产品界面，Popu 只展示摘要和下一步。

## 构建

需要 Rust stable、Windows 10/11 和 WebView2 Runtime。Node.js 仅用于前端语法检查和 npm 命令入口。

```powershell
npm run build
```

也可在安装了 Rust 与 `just` 的 Windows、macOS 或 Linux 环境中运行：

```powershell
just build-test dialog-long-content.json
just build-test dialog-ultra-long-content.json
```

开发期也可直接生成未打包的可执行文件：

```powershell
cargo build --manifest-path src-tauri/Cargo.toml
```

产物位于 `src-tauri/target/debug/popu.exe` 或 `src-tauri/target/release/popu.exe`。

调试 WebView2 时，可在 Debug 构建中设置 `POPU_DEVTOOLS=1` 后运行请求；这会自动打开浏览器开发者工具。该开关在 Release 构建中无效。

## CLI

```powershell
popu request protocol/examples/dialog-confirm.json
popu request protocol/examples/notification-success.json
popu request protocol/examples/notification-custom-template.json --template .\protocol\examples\templates\minimal-notification
popu templates list
popu templates validate --path path/to/my-template
```

布局手工验收可使用 `dialog-unordered-list.json`、`dialog-ordered-list.json`、`dialog-all-elements.json`、`notification-success.json`、`dialog-ultra-long-content.json` 和 `dialog-fullscreen-screen.json`；它们分别验证两种列表、六类内容块与全部表单控件、通知进度、长内容滚动，以及物理屏幕全屏。

可替换模板示例位于 `protocol/examples/templates/minimal-notification`。

Windows PowerShell 5.1 在直接运行通知请求时可能继续等待其 UI 后代关闭，即使 `popu request` 的父进程已经返回。需要立即恢复 PowerShell 提示符时，可使用 Python SDK，或显式异步启动：

```powershell
Start-Process popu -ArgumentList @('request', 'notification.json') -WindowStyle Hidden
```

退出码：`0` 成功或已显示，`10` 取消/关闭，`11` 超时，`12` 请求、模板或运行时错误。stdout 只输出机器可读 JSON，诊断写入 stderr。

## Python

Python SDK 暂未同步本轮 `content.blocks` 协议调整，将在协议稳定后集中更新。当前请通过 JSON 请求或 CLI 使用六类内容块；现有 SDK 仅保留用于回归检查，不应作为新协议示例。

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
