# Popu

Popu 是一个面向自动化脚本和 AI Agent 的轻量本机交互窗口。任务需要人工决定时显示对话框；只需告知结果时显示短暂通知。

当前版本是 Windows 原型：Tauri 2 + WebView2、Rust CLI、Python SDK。它不运行守护进程，不持久化任务，也不做跨调用协调；每次调用拥有独立进程、窗口和结果。

原型阶段默认**不提供向后兼容**：除非需求明确要求，CLI、SDK 和协议接口可以直接重构，调用方应随版本同步更新。

## 功能

- JSON 的 `mode: "dialog"`：等待用户提交、取消、关闭或超时，向 stdout 返回 JSON。
- JSON 的 `mode: "notify"`：窗口创建后返回，通知到期自动退出。
- 内置 `confirm`、`form`、`notification` 三种模板。
- 可信本地 HTML/CSS 模板替换；模板不执行自带 JavaScript。
- 鼠标所在屏幕居中或工作区右下角定位。
- 无边框透明圆角窗口；Windows 圆角外部区域鼠标穿透到底层窗口。
- `data-popu-drag-region` 定义拖拽背景，`data-popu-selectable` 定义可复制文本；内置模板默认允许复制正文和详情。
- Python lifetime pipe：Python 调用方退出时关闭对应 Popu 窗口。

## 构建

需要 Rust stable、Windows 10/11 和 WebView2 Runtime。Node.js 仅用于前端语法检查和 npm 命令入口。

```powershell
npm run build
```

开发期也可直接生成未打包的可执行文件：

```powershell
cargo build --manifest-path src-tauri/Cargo.toml
```

产物位于 `src-tauri/target/debug/popu.exe` 或 `src-tauri/target/release/popu.exe`。

## CLI

```powershell
popu request protocol/examples/dialog-confirm.json
popu request protocol/examples/notification-success.json
popu request protocol/examples/notification-custom-template.json --template .\protocol\examples\templates\minimal-notification
popu templates list
popu templates validate --path path/to/my-template
```

布局手工验收可使用 `protocol/examples/dialog-short-content.json`、`dialog-long-content.json` 和 `dialog-ultra-long-content.json`；它们分别验证紧凑短内容、长文本/详情/表单，以及当前 2000px 高度上限下的内容溢出与滚动行为。

可替换模板示例位于 `protocol/examples/templates/minimal-notification`。

Windows PowerShell 5.1 在直接运行通知请求时可能继续等待其 UI 后代关闭，即使 `popu request` 的父进程已经返回。需要立即恢复 PowerShell 提示符时，可使用 Python SDK，或显式异步启动：

```powershell
Start-Process popu -ArgumentList @('request', 'notification.json') -WindowStyle Hidden
```

退出码：`0` 成功或已显示，`10` 取消/关闭，`11` 超时，`12` 请求、模板或运行时错误。stdout 只输出机器可读 JSON，诊断写入 stderr。

## Python

```powershell
python -m pip install -e ./python
```

```python
from popu import DialogRequest, Notification, notify, show

result = show(DialogRequest.confirm(title="继续执行吗？"))
if result.status == "submitted" and result.action == "confirm":
    print("confirmed")

notify(Notification.success(title="任务完成", message="共处理 128 个文件"))
```

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
