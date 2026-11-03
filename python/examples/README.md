# Python SDK 演示

`progress.py` 模拟后台备份，不读写备份文件。安装[本地 Windows wheel](../../docs/python-distribution.md)后可直接运行，默认使用包内运行时：

```powershell
python python/examples/progress.py --wait
```

源码开发则使用 editable SDK 和显式指定的同版本 Popu 可执行文件：

```powershell
python -m pip install -e ./python
cargo build --manifest-path src-tauri/Cargo.toml
python python/examples/progress.py --executable src-tauri/target/debug/popu.exe --wait
```

默认每 3 秒提交一次进度，共 36 秒。可以拖动卡片、点击 × 收起；普通进度不会突破主动关闭，最后一次更新会提交摘要、关闭更新入口，并申请一次再次展示。

每次进度更新都会重新获得完整展示时长。默认展示 20 秒，因此每 3 秒更新时卡片会持续显示，最后一份摘要按 `--final-seconds` 单独计时。

在启动、进度等待、更新或 `--wait` 阶段按 Ctrl+C，示例会打印停止信息并以退出码 130 结束。未完成的通知会尝试使用最新 revision 提交停止摘要、关闭更新入口；查询和更新各最多等待 1 秒，失败则跳过收尾。已经提交的完成摘要会保留。再次按 Ctrl+C 可以中断收尾，常驻 Host 不会被停止。

自动超时后再展示：

```powershell
python python/examples/progress.py --executable src-tauri/target/debug/popu.exe --display-seconds 5 --wait
```

快速演示：

```powershell
python python/examples/progress.py --executable src-tauri/target/debug/popu.exe --interval 2 --steps 4 --display-seconds 3 --final-seconds 5 --wait
```

演示结束后，从托盘打开历史查看最终摘要。若常驻 Host 来自旧版本，请先退出旧 Host，再运行演示。

若使用本次验证构建，可将 `--executable` 替换为 `src-tauri/target/live-notifications/debug/popu.exe`。不安装 SDK 时，可先执行 `$env:PYTHONPATH = "$PWD/python/src"`，从当前源码加载 SDK。
