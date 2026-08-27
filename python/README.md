# Popu Python SDK

Popu 的轻量 Python 封装。SDK 负责生成请求、启动 `popu` 可执行文件、解析响应，并通过 lifetime pipe 保证 Python 进程退出时对应窗口一同结束。

```python
from popu import DialogRequest, show

result = show(DialogRequest.confirm(title="Continue?"))
```

`popu.exe` 默认从 `PATH` 查找，也可使用 `Client(executable=...)` 显式指定。SDK 统一调用 `popu request <REQUEST>`，由请求 JSON 的 `mode` 决定对话框或通知行为。
