# Python 分发与阶段审核

目标是通过一个 `popu` Windows wheel 安装原生工具和 Python SDK。当前只实现第一阶段，尚未向 PyPI 发布。

## 阶段划分

| 阶段 | 范围 | 当前状态 |
| --- | --- | --- |
| 1 | 本地 wheel、命令入口、SDK 默认解析、不可变运行时缓存与安装验证 | 已实现，等待审核 |
| 2 | CLI 与已有 Host 的版本/协议握手，WebView2 缺失诊断和手动安装入口 | 待第一阶段审核 |
| 3 | 发布 workflow、发行版本检查、制品验证和 PyPI Trusted Publisher | 待第二阶段审核 |

每阶段完成后停下审核。仓库现有 `.github/workflows/python-publish.yml` 留到第三阶段修改；当前不能直接依赖它发布本包。

## 第一阶段的接口

- 分发名和导入名都是 `popu`，wheel 标签为 `py3-none-win_amd64`，要求 64 位 Python 3.10+；不发布 sdist，也不在安装时编译 Rust。
- 安装后提供 `popu` 和 `python -m popu`。命令入口转发参数、标准输入、标准输出、标准错误和原生退出码。运行时准备失败输出到 stderr，退出码为 12；Ctrl+C 退出码为 130，只停止前台 CLI。
- `Client()` 默认使用包内的匹配版本，不搜索 `PATH`。源码开发和特殊部署继续支持 `Client(executable=...)`。
- SDK 版本由 `python/src/popu/_version.py` 提供。打包前检查它与 Cargo、Tauri、npm 和 `popu --version` 一致。
- wheel 内包含 `popu.exe`、许可证和 SHA-256 清单。构建检查 Windows x64 PE 类型、文件清单和校验值；可显式传入额外的 x64 DLL。

## 本地构建和安装

构建机器需要 Windows x64、Python 3.11+、Rust、Node.js 和前端依赖。安装后的 SDK 不依赖构建工具。以下命令在仓库根目录执行：

```powershell
npm ci
npm run build
python scripts/stage-python-runtime.py
python -m pip install build
python -m build --wheel --outdir dist python

python -m venv test-results/popu-review
./test-results/popu-review/Scripts/python.exe -m pip install --no-index --no-deps ./dist/popu-0.1.0-py3-none-win_amd64.whl
./test-results/popu-review/Scripts/popu.exe --version
./test-results/popu-review/Scripts/python.exe -m popu templates list
./test-results/popu-review/Scripts/python.exe python/examples/backup_progress.py
```

若需额外依赖库，可在 staging 时重复传入 `--dll <path>`；必须为 x64 PE DLL。staging 输出位于被 Git 忽略的 `python/src/popu/_native/`，wheel 输出位于 `dist/`。未准备或损坏的原生载荷会阻止 wheel 构建。源码 editable 安装允许不含载荷，但这时必须显式指定本地可执行文件。

## 运行时缓存

首次默认调用校验包内清单，并将载荷复制到：

```text
%LOCALAPPDATA%\Popu\runtime\<version>-win_amd64-<manifest-sha256-prefix>\popu.exe
```

若 `LOCALAPPDATA` 未设置，则使用用户目录下的 `AppData\Local`。并发调用各自准备临时目录，校验后通过目录重命名发布；其他调用只使用完整目录。每次解析校验已缓存文件，损坏时明确报错，不覆盖可能正在运行的文件。

CLI 和由其启动的 Host 均从该缓存运行，避免 Host 长驻时锁住 `site-packages` 中的文件。升级会选择新的版本或清单目录，旧目录保留；卸载 Python 包不会停止 Host 或删除缓存。缓存自动回收暂未实现。

## 审核边界

第一阶段验证本地 wheel 安装和启动路径、参数与输入输出转发、退出码、Ctrl+C、缓存并发、损坏拒绝和旧文件保留。它不代表已通过干净 Windows 虚拟机的全部系统依赖验证。

本次本机验证结果：66 项 Python 测试通过；隔离 venv 安装 wheel 后，两个命令入口和默认 SDK 路径正常，中文缓存路径和标准输入正常，原生校验失败返回退出码 12。保持缓存文件打开时重装成功，卸载后缓存保留，再次安装可用。wheel 的平台标签、入口、许可证和文件校验值通过检查；缺失载荷、版本不一致、校验失败、非 x64 PE 和清单外文件均会阻止构建。原先跟踪的 `*.egg-info` 生成元数据已移除并忽略，后续由构建工具生成。

已有 Host 仍遵循当前固定 IPC 入口，尚未拒绝不兼容的版本；第二阶段补充握手，因此第一阶段不应直接作为公开发行版。WebView2 仍由用户手动安装，第二阶段添加缺失时的清晰提示及 [Microsoft 安装入口](https://developer.microsoft.com/microsoft-edge/webview2/)，不会自动补齐。
