# Popu

[English](https://github.com/ZeroKwok/Popu/blob/master/python/README.en.md)

为 Python 脚本和自动化工具提供桌面通知与轻量对话框。

展示任务进度、汇报结果、请求确认，并在脚本退出后保留通知历史。安装包同时提供 Python SDK 和 `popu` 命令行工具，全部在本机运行，无需外部服务或账户。

## 安装与体验

目前支持 **Windows x64、64 位 Python 3.10+**。需要 [Microsoft WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)；如果缺失，Popu 会提示手动安装入口。

```sh
python -m pip install popu
python -m popu.demo progress
```

演示先展示任务进度，再显示结果摘要。它模拟一个短任务，不会修改你的文件。可尝试关闭进度卡片：后台更新仍会继续，最终摘要会请求再展示一次。按 Ctrl+C 停止演示；加 `--wait` 可等待最终通知结束展示。弹窗和控制台提示均使用英文。

安装包已包含原生可执行文件。无需检出仓库或安装构建工具，可在任意目录使用。后台 Host 会在首次请求时自动启动；提交脚本退出后，通知和历史仍由 Host 管理。

## 发送通知

```python
from popu import Client, Notification

client = Client()
client.notify(
    Notification.success(
        title="Backup completed",
        message="128 files backed up successfully.",
    )
)
```

## 请求确认

```python
from popu import Client, DialogRequest

result = Client().show(
    DialogRequest.confirm(
        title="Deploy release?",
        message="The release is ready for production.",
    )
)

if result.status == "submitted" and result.action == "confirm":
    print("Deployment confirmed")
```

`show()` 等待用户响应。取消、关闭或超时都是正常结果，执行后续任务前应检查返回值。

## 更新任务进度

此示例使用模拟进度；实际集成时，将等待和百分比替换为任务自身的上报。

```python
import time
from popu import Client

client = Client()
receipt = client.notify({
    "version": "1.0",
    "mode": "notify",
    "template": "notification",
    "updatable": True,
    "content": {
        "severity": "info",
        "title": "Example task",
        "blocks": [{"type": "progress", "value": 0}],
    },
})
revision = receipt.revision

for percent in (25, 50, 75, 100):
    time.sleep(1)
    done = percent == 100
    snapshot = client.update(
        receipt.request_id,
        revision,
        {
            "severity": "success" if done else "info",
            "title": "Example completed" if done else "Example task",
            "blocks": [{"type": "progress", "value": percent}],
        },
        updatable=not done,
        reshow=done,
    )
    revision = snapshot["revision"]
```

- 创建需要更新的通知时，设置 `updatable=True`。
- 每次更新替换完整内容，并使用最新 revision。
- 最后设置 `updatable=False`，保存最终内容并关闭更新入口。
- 成功更新会重新开始展示计时。主动关闭卡片后，普通更新保持静默；`reshow=True` 可显式请求再次展示，默认允许初次展示之外再展示一次。自动超时后，下次普通更新可以再次展示。

完整规则见[通知生命周期](https://github.com/ZeroKwok/Popu/blob/master/docs/live-notifications.md)。

## 使用命令行

```sh
popu --help
popu templates list
```

将以下请求保存为 `notification.json`：

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

CLI 和 SDK 共用本地 Host 与通知历史。其他请求类型见 [JSON 协议](https://github.com/ZeroKwok/Popu/blob/master/protocol/popu.schema.json)。

## 常见问题

- **找不到 `popu` 命令**：使用参数相同的 `python -m popu`，或将 Python scripts 目录加入 `PATH`。
- **缺少 WebView2**：从 [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/) 安装运行时后重试。Popu 不会自动安装。
- **升级后仍有旧 Host 运行**：通过 `python -m popu host status` 查询。确认其中的交互可以结束后，从托盘退出或执行 `python -m popu host stop`，再重试。下次请求会启动新 Host。

升级安装包可执行 `python -m pip install --upgrade popu`。

## 文档导航

- [SDK 参考](https://github.com/ZeroKwok/Popu/blob/master/docs/python-sdk.md)：请求结果、窗口选项、图片、图表与运行时行为。
- [示例指南](https://github.com/ZeroKwok/Popu/blob/master/python/src/popu/demo/README.md)：可用示例、参数与源码开发。
- [项目介绍](https://github.com/ZeroKwok/Popu#readme)和[问题反馈](https://github.com/ZeroKwok/Popu/issues)。
- [构建与发布](https://github.com/ZeroKwok/Popu/blob/master/docs/python-distribution.md)：面向从源码构建的贡献者。

## 许可证

[Apache License 2.0](https://github.com/ZeroKwok/Popu/blob/master/LICENSE)。
