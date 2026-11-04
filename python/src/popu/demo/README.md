# Python SDK 示例

本目录保存随 `popu` 包提供的示例及其文档。安装后可在任意目录运行，无需检出仓库或安装编译工具。目前仅支持 Windows x64、64 位 Python 3.10+，需要 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。

## 安装与运行

```sh
python -m pip install popu
python -m popu.demo progress
```

可选加上 `--wait`，等待最终通知结束展示后再退出：

```sh
python -m popu.demo progress --wait
```

查看可用示例和各示例的参数：

```sh
python -m popu.demo --help
python -m popu.demo progress --help
```

所有示例的控制台提示使用英文。

## 示例列表

| 命令 | 示例 | 说明 |
| --- | --- | --- |
| `python -m popu.demo progress` | [进度与最终摘要](#进度与最终摘要) | 模拟任务进度、后台更新、关闭后的再次展示和结果历史 |

## 进度与最终摘要

实现位于 [progress.py](progress.py)。它以模拟备份场景展示通知，不会读取、复制、修改或校验用户的备份文件。通知内容明确标记为模拟，控制台会显示通知 ID、进度、状态和 revision。

默认每 3 秒更新，共 12 步，约 36 秒后提交最终摘要。可以拖动卡片、点击关闭按钮收起；普通更新静默保存，最后一次更新提交摘要、关闭更新入口，并申请一次再次展示。完成后可从托盘打开历史查看结果。

每次成功更新都会重新获得完整展示时长。默认展示 20 秒，最终摘要默认展示 10 秒。`--wait` 仅决定是否等待最终通知结束展示，不影响后台 Host 和通知的生命周期。

在启动、进度等待、更新或最终等待阶段按 Ctrl+C，示例以退出码 130 结束。未完成的通知会尝试提交停止摘要并关闭更新入口，查询和更新各最多等待 1 秒；再次按 Ctrl+C 可中断收尾。已经提交的完成摘要会保留，常驻 Host 不会被停止。

### 调整演示时间

以下是可选的进阶参数，基础运行无需指定它们。

更快完成：

```sh
python -m popu.demo progress --interval 1 --steps 5
```

观察自动超时后下次更新再次展示：

```sh
python -m popu.demo progress --interval 6 --display-seconds 5 --wait
```

`--final-seconds` 可单独调整最终摘要的展示时间。全部选项见 `python -m popu.demo progress --help`。

## 源码开发

在仓库根目录执行：

```sh
python -m pip install -e ./python
just build
just demo-python --wait
```

`just demo-python` 运行同一份包内进度示例，显式选择 Cargo 当前构建目录中的 debug 程序，支持 `CARGO_TARGET_DIR`，可透传示例参数。也可使用 `--executable` 指定同版本的本地程序；`--help` 不需要先构建原生程序。

如果已有旧版本 Host，请先确认其中的交互可以结束，再从托盘退出或执行 `python -m popu host stop`，然后运行新版本示例。

### 添加示例

在本目录新增模块，提供 `main(argv=None)` 入口，并在 [__main__.py](__main__.py) 的 `DEMOS` 中注册。模块文档首行用于示例列表的帮助说明；模块自行解析参数，控制台提示保持英文。

新增后同步更新本页示例列表，用户统一通过 `python -m popu.demo <示例名>` 运行。通用安装说明和命令入口由本目录统一维护。
