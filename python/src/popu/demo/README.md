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

实现位于 [progress.py](progress.py)，用于说明最基本的通知更新流程：

1. 创建 `updatable: true` 的通知，并保存回执中的请求 ID 和 revision。
2. 每秒上报一次进度，使用返回的最新 revision 提交下一次完整内容。
3. 约 5 秒后单独提交 100% 的最终摘要，通过 `updatable: false` 关闭更新入口，并用 `reshow: true` 请求再次展示。
4. 可选使用 `--wait` 等待最终通知结束展示。

这是通用的模拟任务，不执行真实文件操作。弹窗与控制台均使用英文，控制台仅展示进度、完成或停止提示。阅读示例时，可以将循环中的等待和百分比替换为自己的任务。

进度通知沿用默认展示时长，每次成功更新重新计时；最终摘要展示 10 秒。可以尝试中途关闭卡片：任务继续上报，普通更新静默保存，最终摘要会请求一次再次展示。完成后可从托盘打开历史查看结果。

按 Ctrl+C 会以退出码 130 停止演示。若已取得回执，会尝试查询最新 revision，并为仍可更新的通知提交英文停止摘要。查询和更新各最多等待 1 秒，再次按 Ctrl+C 可中断收尾。已经提交的最终摘要会保留，Host 不会被停止。

示例仅保留 `--wait` 和开发用的 `--executable` 参数，时序固定；参数边界和中断时序由测试覆盖。

## 源码开发

在仓库根目录执行：

```sh
just setup-python
just demo-python --wait
```

`just setup-python` 完成 debug 构建、开发载荷同步和 editable 安装，之后也可直接运行 `python -m popu.demo progress --wait`。修改 CLI/Host 后重新执行此配方即可。

`just demo-python` 运行同一份包内进度示例，显式选择 Cargo 当前构建目录中的 debug 程序，支持 `CARGO_TARGET_DIR`，可透传示例参数。也可使用 `--executable` 指定同版本的本地程序；`--help` 不需要先构建原生程序。

如果已有旧版本 Host，请先确认其中的交互可以结束，再从托盘退出或执行 `python -m popu host stop`，然后运行新版本示例。

### 添加示例

在本目录新增模块，提供 `main(argv=None)` 入口，并在 [__main__.py](__main__.py) 的 `DEMOS` 中注册。模块文档首行用于示例列表的帮助说明；模块自行解析参数，控制台提示保持英文。

新增后同步更新本页示例列表，用户统一通过 `python -m popu.demo <示例名>` 运行。通用安装说明和命令入口由本目录统一维护。
