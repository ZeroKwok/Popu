# Python SDK 演示

进度演示的唯一实现位于 `popu.demo.progress`，随 Python 包一起安装。它使用模拟备份场景展示通知、进度更新和结果摘要；文件数量与数据量均为模拟，不会读取、复制、修改或校验用户的备份文件。安装[本地 Windows wheel](../../docs/python-distribution.md)后，在任意目录运行，默认使用包内运行时：

```powershell
python -m popu.demo progress --wait
```

快速测试和帮助：

```powershell
python -m popu.demo progress --interval 1 --steps 5 --wait
python -m popu.demo --help
python -m popu.demo progress --help
```

运行不需要仓库、just、Cargo 或 Node.js。通知标题与终端输出明确显示“模拟进度演示”，最终摘要和中断摘要也会注明未执行真实备份。默认每 3 秒更新、共 12 步，约 36 秒后提交最终摘要；`--wait` 会继续等待最终通知展示结束。

源码开发则使用 editable SDK 和显式指定的同版本 Popu 可执行文件：

```powershell
python -m pip install -e ./python
just build
just demo-python --wait
```

`just demo-python` 调用同一份包内演示，并选择 Cargo 当前构建目录中的 debug 程序，支持 `CARGO_TARGET_DIR`，可透传 `--interval`、`--steps` 等参数。首次使用时，先按上面的命令安装 editable SDK 并构建本地程序。可用 `--executable` 显式覆盖程序路径，`--help` 无需先构建原生程序。启动工具在同一 Python 进程执行示例，保留 Ctrl+C 的停止摘要与有界收尾行为。

默认每 3 秒提交一次进度，共 36 秒。可以拖动卡片、点击 × 收起；普通进度不会突破主动关闭，最后一次更新会提交摘要、关闭更新入口，并申请一次再次展示。

每次进度更新都会重新获得完整展示时长。默认展示 20 秒，因此每 3 秒更新时卡片会持续显示，最后一份摘要按 `--final-seconds` 单独计时。

在启动、进度等待、更新或 `--wait` 阶段按 Ctrl+C，示例会打印停止信息并以退出码 130 结束。未完成的通知会尝试使用最新 revision 提交停止摘要、关闭更新入口；查询和更新各最多等待 1 秒，失败则跳过收尾。已经提交的完成摘要会保留。再次按 Ctrl+C 可以中断收尾，常驻 Host 不会被停止。

自动超时后再展示：

```powershell
just demo-python --display-seconds 5 --wait
```

快速演示：

```powershell
just demo-python --interval 2 --steps 4 --display-seconds 3 --final-seconds 5 --wait
```

演示结束后，从托盘打开历史查看最终摘要。若常驻 Host 来自旧版本，请先退出旧 Host，再运行演示。

若使用本次验证构建，可将 `--executable` 替换为 `src-tauri/target/live-notifications/debug/popu.exe`。不安装 SDK 时，先执行 `just prep-python`，再设置 `$env:PYTHONPATH = "$PWD/python/src"`，从当前源码加载 SDK。
