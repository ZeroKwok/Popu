# Python SDK 参考

[English](python-sdk.en.md)

安装、可运行示例和常见问题见 [Python 包入门](../python/README.md)。本参考说明请求处理、通知生命周期、窗口选项、图像与图表，以及运行时行为。

## 请求与结果

```python
from popu import Client, DialogRequest, Notification

client = Client()
result = client.show(DialogRequest.confirm(title="Continue?"))

receipt = client.notify(Notification.success(title="Backup running", updatable=True))
snapshot = client.status(receipt.request_id)
snapshot = client.update(receipt.request_id, receipt.revision, {
    "severity": "success",
    "title": "Backup 50%",
    "blocks": [],
})
client.dismiss(receipt.request_id)
```

`create` 和 `notify` 返回 `RequestReceipt`（`NotificationReceipt` 为其别名），包含 `status="accepted"`、Host 生成的 `request_id`、`state`、`revision` 和 `duplicate`。接受时请求可能仍处于 `queued`，不代表窗口已经展示。无论通知还是对话框，`create` 都在请求被接受后返回；`show` 会继续等待，并返回包含 `status`、`action` 和 `values` 的 `InteractionResult`。

`status`、`update`、`dismiss` 和 `wait` 返回 Host 快照字典。`wait` 读取权威终态快照，包括 revision、内容、时间戳、响应和退出码；隐藏的可更新通知尚未进入终态。其他客户端也可以通过请求 ID 管理同一请求。更新必须使用最新 revision，并替换完整内容。每次成功的通知更新都会按配置的展示时长重新计时，可用 `duration_ms` 覆盖该时长。对话框更新保留原始交互截止时间。revision 冲突、ID 不存在、校验失败、更新通道已关闭，以及子进程或 JSON 错误都会抛出 `PopuError`。

## 可更新通知

通知默认 `updatable=False`，不接受更新。进度上报需要在创建时显式开启更新。更新时省略此选项会保留当前值；最后一次上报设置 `updatable=False`，原子保存最终内容并拒绝后续更新。不使用 `final` 字段。对话框内容更新不使用通知专用选项。

```python
receipt = client.notify(Notification.success(title="Backing up", updatable=True))
snapshot = client.update(receipt.request_id, receipt.revision, {
    "title": "Backup 50%", "blocks": [{"type": "progress", "value": 50}],
})
snapshot = client.update(receipt.request_id, snapshot["revision"], {
    "severity": "success", "title": "Backup completed",
    "blocks": [{"type": "paragraph", "text": "2,000 files verified."}],
}, updatable=False, reshow=True, duration_ms=10000)
```

用户主动关闭会收起进度并使普通更新保持静默。显式传入 `reshow=True`，默认允许额外展示一次；次数耗尽时通过 `reshow_suppressed` 告知，但仍接受内容更新。自动展示超时也会收起卡片，但只要用户之前没有主动关闭，下次上报就会再次展示。`dismiss` 表示调用方取消，会直接结束通知。

默认模板展示通知发生时间，每次成功上报都会刷新。隐藏内容仍在历史中保持最新。连续 12 小时没有成功上报后，通知进入 `stale` 状态并拒绝更新。Host 重启后将活动记录恢复为隐藏状态，不重置该截止时间。完整规则见[通知生命周期](https://github.com/ZeroKwok/Popu/blob/HEAD/docs/live-notifications.md)，可运行示例见[包内进度演示](https://github.com/ZeroKwok/Popu/blob/HEAD/python/src/popu/demo/README.md)。

对话框取消、关闭或超时都是正常的 `InteractionResult` 结果。自定义动作可使用 `12` 或其他 0–255 范围内的退出码；SDK 读取 Host 结果，不会将这种业务退出码当作传输错误。对话框终态没有响应时，`show` 会抛出包含终态信息的 `PopuError`。

`Client.create`、`Client.show`、`Client.notify` 及模块级 `show` / `notify` 辅助函数均接受 `template=Path(...)`，指定可信的自定义模板目录。默认客户端使用包内匹配版本，不搜索 `PATH`；可通过 `Client(executable=...)` 显式覆盖。CLI 使用 stdout 管道传输 UTF-8 JSON 行，不创建 stdout 临时文件或轮询回执。通知和管理命令先读取响应行，再等待 CLI 退出，不等待管道尾部 EOF。stderr 使用临时文件，避免大量诊断阻塞回执。对话框被接受后，即使调用方从未执行 `wait`，SDK 也会在后台以有界块排空剩余 stdout 并回收 CLI；请求生命周期由 Host 管理，Host 启动时不继承 CLI 句柄。

Windows 上的响应读取和进程等待可以被中断：阻塞管道读取由工作线程执行，调用线程使用有界等待以分发 Ctrl+C。中断只结束并回收当前 CLI，然后向上传递 `KeyboardInterrupt`，不会停止 Host 或隐式取消请求。SDK 启动的 Windows CLI 不带控制台，由 Python 调用方处理 Ctrl+C。交互式 `popu` / `python -m popu` 入口则保留终端控制台和标准流，并让原生进程使用独立进程组，仍由 Python 负责 Ctrl+C 清理。

`Client(timeout=2)` 可选地将每次同步 CLI 操作限制为两秒，包括读取响应与等待进程退出。默认不设截止时间。超时会抛出 `PopuError` 并清理 CLI。对话框提交截止时间在请求被接受后结束，不限制后续后台读取，也不替代 `timeout_seconds`。显式配置的客户端截止时间同样适用于前台 `wait` 调用。

## 窗口选项

通知始终使用 `window.always_on_top=True`；`Notification.success` 不提供取消置顶的选项。默认使用主显示器工作区，并拒绝 `area=ScreenArea.SCREEN`。原始请求字典由 CLI / Host 校验，同样拒绝通知的 `always_on_top=False`。

对话框构造器支持 `always_on_top`、`screen`、`area`、`width` 和 `height`。尺寸使用逻辑像素或百分比字符串，无需手工按 DPI 放大。两类构造器均支持 `vertical_align` 和 `coalesce_key`：

```python
from popu import Screen, ScreenArea, VerticalAlign

request = DialogRequest.confirm(
    title="Review", always_on_top=False,
    screen=Screen.PRIMARY, area=ScreenArea.SCREEN,
    width="80%", height="80%", vertical_align=VerticalAlign.START,
)
receipt = client.notify(Notification.success(title="Backup", coalesce_key="job:backup"))
```

其他严重级别、表单或自定义对话框动作，可通过完整协议请求字典传给 `create`、`show` 或 `notify`，无需另行实现 Python 传输层或渲染器。

## 阅读要求

通知接受 `requires_read=True`，默认值为 `False`：

```python
client.notify(Notification.success(title="Review backup results", requires_read=True))
```

超时后，通知以未读状态保留在历史中，但只有声明阅读要求的通知会持续触发托盘提醒。已展示或排队的通知不会触发该提醒；它只代表历史中仍需阅读的未读内容。前台历史窗口中查看单条详情会将其标为已读，打开窗口不会批量确认整个列表。阅读不改变请求终态。对话框不接受此字段。

两类构造器均接受 `blocks=[...]` 和可选的 `icon` 图像源。使用 `image_source` 读取本地 SVG、PNG、JPEG 或 WebP 文件，或使用 `svg_source` 提交 SVG 文本；提交的数据是自包含的，在历史中仍可使用。

```python
from popu import chart_block, image_block, image_source, svg_source

chart = chart_block({
    "data": {"values": [{"service": "Sync", "ms": 115}, {"service": "Backup", "ms": 78}]},
    "mark": "bar",
    "encoding": {
        "x": {"field": "service", "type": "nominal"},
        "y": {"field": "ms", "type": "quantitative"},
    },
}, title="Service latency", height=240)
client.notify(Notification.success(title="Measurements", blocks=[chart], height=400))
client.show(DialogRequest.confirm(title="Review", blocks=[chart], width=740, height=580))

# A caller-rendered graphic, including chart types outside Vega-Lite:
picture = image_block(image_source("report.svg", alt="Report graphic"), height=240)
client.notify(Notification.success(title="Report", blocks=[picture], height=400))
```

图表块接受包含内联数据的通用 Vega-Lite 6 规格，离线渲染为可交互 SVG。默认悬浮提示展示编码字段值；可通过 `encoding.tooltip` 自定义标签和格式，或设为 `null` 禁用。规格中声明 Vega-Lite 原生 `params` 时，支持最近点悬浮、高亮、图例选择以及绑定坐标轴缩放和平移。编译在本地 worker 中执行，兼容 CSP 的实时 Vega 视图负责渲染和交互。图片块仍为静态内容，修改图表 mark 或编码即可展示折线、柱状图和热力图。图像与图表的 `height` 范围为 64–1200；图像 data URI 上限为 256 KiB，图表规格上限为 128 KiB，完整内容上限为 768 KiB。SVG 中的脚本与外部资源会被拒绝。

## 运行时与源码开发

源码开发使用 editable 安装：

```powershell
python -m pip install -e ./python
```

通过 `Client(executable="path/to/popu.exe")` 使用本地构建，也接受 `["path/to/popu.exe"]` 形式的命令序列。editable 安装不会构建或下载原生程序。SDK 面向当前 Host 协议，不接受引入 Host 之前的响应格式。

产品版本只在 `src-tauri/Cargo.toml` 中维护。构建和 editable 安装自动生成 `_version.py`，安装后的 SDK 可独立使用。`just test-python` 也会在测试前准备版本。直接通过 `PYTHONPATH` 使用未安装的源码时，先运行 `just prep-python`。生成构建元数据需要完整仓库；安装用户应使用 wheel。

默认情况下，CLI 和 SDK 校验包内清单，再将载荷复制到 `%LOCALAPPDATA%\Popu\runtime\<version>-win_amd64-<manifest-hash>\`。原生进程从此不可变缓存运行，避免长驻 Host 在升级或卸载时锁住 `site-packages` 内的文件。旧缓存会保留，目前未实现自动清理。

提交业务请求前，原生 CLI 和 Host 在同一连接上握手，要求应用版本与 RPC 版本完全一致。旧版或不兼容 Host 会在提交前返回诊断，Python 抛出 `PopuError`。可通过 `python -m popu host status` 查询；确认可以结束当前交互后，再执行 `python -m popu host stop` 或从托盘退出。Popu 不会自动停止已有 Host。管理命令无需业务握手即可使用。握手有两秒截止时间，不限制对话框的业务等待。已提交业务请求在传输失败后不会自动重放。

启动新 Host 前，CLI 会检查 WebView2 可用性。运行时缺失或不可用时，提供手动安装链接和 loader 诊断，不会等待到后台启动超时。这项检查不安装软件，也不打开浏览器。包校验和发布操作见[构建与发布指南](https://github.com/ZeroKwok/Popu/blob/HEAD/docs/python-distribution.md)。
