# Python SDK 演示

`backup_progress.py` 模拟后台备份，不读写备份文件。需要安装本项目 SDK，并使用同版本的 Popu 可执行文件。

```powershell
python -m pip install -e ./python
cargo build --manifest-path src-tauri/Cargo.toml
python python/examples/backup_progress.py --executable src-tauri/target/debug/popu.exe --wait
```

默认每 15 秒提交一次进度，共 90 秒。可以拖动卡片、点击 × 收起；普通进度不会突破主动关闭，最后一次更新会提交摘要、关闭更新入口，并申请一次再次展示。

自动超时后再展示：

```powershell
python python/examples/backup_progress.py --executable src-tauri/target/debug/popu.exe --display-seconds 5 --wait
```

快速演示：

```powershell
python python/examples/backup_progress.py --executable src-tauri/target/debug/popu.exe --interval 2 --steps 4 --display-seconds 3 --final-seconds 5 --wait
```

演示结束后，从托盘打开历史查看最终摘要。若常驻 Host 来自旧版本，请先退出旧 Host，再运行演示。

若使用本次验证构建，可将 `--executable` 替换为 `src-tauri/target/live-notifications/debug/popu.exe`。不安装 SDK 时，可先执行 `$env:PYTHONPATH = "$PWD/python/src"`，从当前源码加载 SDK。
