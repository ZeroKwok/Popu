# Python 分发与阶段审核

目标是通过一个 `popu` Windows wheel 安装原生工具和 Python SDK。当前已实现前两个阶段，第二阶段等待审核，尚未向 PyPI 发布。

## 阶段划分

| 阶段 | 范围 | 当前状态 |
| --- | --- | --- |
| 1 | 本地 wheel、命令入口、SDK 默认解析、不可变运行时缓存与安装验证 | 已审核 |
| 2 | CLI 与已有 Host 的版本/协议握手，WebView2 缺失诊断和手动安装入口 | 已实现，等待审核 |
| 3 | 发布 workflow、发行版本检查、制品验证和 PyPI Trusted Publisher | 待第二阶段审核 |

每阶段完成后停下审核。仓库现有 `.github/workflows/python-publish.yml` 留到第三阶段修改；当前不能直接依赖它发布本包。

## 第一阶段的接口

- 分发名和导入名都是 `popu`，wheel 标签为 `py3-none-win_amd64`，要求 64 位 Python 3.10+；不发布 sdist，也不在安装时编译 Rust。
- 安装后提供 `popu` 和 `python -m popu`。命令入口转发参数、标准输入、标准输出、标准错误和原生退出码。运行时准备失败输出到 stderr，退出码为 12；Ctrl+C 退出码为 130，只停止前台 CLI。
- `Client()` 默认使用包内的匹配版本，不搜索 `PATH`。源码开发和特殊部署继续支持 `Client(executable=...)`。
- 产品版本唯一来源是 `src-tauri/Cargo.toml` 的 `package.version`。Python 构建、editable 安装和载荷准备自动生成被 Git 忽略的 `python/src/popu/_version.py`；Tauri 自动继承 Cargo，UI npm 包不再保存产品版本。打包仍检查 `popu --version`，拒绝混入旧版本二进制。
- wheel 内包含 `popu.exe`、许可证和 SHA-256 清单。构建检查 Windows x64 PE 类型、文件清单和校验值；可显式传入额外的 x64 DLL。

## 本地构建和安装

构建机器需要 Windows x64、Python 3.11+、Rust、Node.js、just、ImageMagick 和前端依赖。安装后的 SDK 不依赖构建工具。以下命令在仓库根目录执行（建议使用开发虚拟环境）：

```powershell
just setup
python -m pip install build
just wheel

python -m venv test-results/popu-review
./test-results/popu-review/Scripts/python.exe -m pip install --no-index --no-deps ./dist/popu-0.1.1-py3-none-win_amd64.whl
./test-results/popu-review/Scripts/popu.exe --version
./test-results/popu-review/Scripts/python.exe -m popu templates list
./test-results/popu-review/Scripts/python.exe -m popu.demo progress --wait
```

若需额外依赖库，可在 staging 时重复传入 `--dll <path>`；必须为 x64 PE DLL。staging 输出位于被 Git 忽略的 `python/src/popu/_native/`，wheel 输出位于 `dist/`。未准备或损坏的原生载荷会阻止 wheel 构建。源码 editable 安装允许不含载荷，但这时必须显式指定本地可执行文件。

`just wheel` 依次构建 `ui/dist/`、原生 release 程序、版本化载荷和 wheel。UI 的依赖、脚本和测试不会进入 Tauri 嵌入资源。若只想更新 `_native`，执行 `just build-release` 后运行 `python scripts/stage-python-runtime.py`。只修改 Cargo 中的版本后，需重新编译原生程序；载荷准备不会自动拿旧 exe 冒充新版本。SDK 安装后使用随包生成的版本文件，不读取仓库中的 Cargo 清单。

原生构建会先按需准备图标。载荷准备默认从 Cargo metadata 读取 release 程序位置，尊重 `CARGO_TARGET_DIR`，也可用 `--executable` 显式覆盖。当前 wheel 配方及载荷准备仅支持 Windows；Python 解释器可通过 `PYTHON` 环境变量指定。

wheel 包含 `popu.demo.progress` 的唯一实现。安装后可在任意目录执行 `python -m popu.demo progress --wait`；快速测试使用 `--interval 1 --steps 5`。这是模拟进度演示，模拟备份只是表现情境，不操作用户备份文件。仓库开发入口 `just demo-python` 调用同一实现，并使用当前 Cargo debug 程序；没有仓库外层的重复示例文件。

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

## 第二阶段：Host 兼容性与运行依赖

CLI 连接固定 IPC 入口后，先发送 `host.hello`，双方检查应用版本与 RPC 版本完全一致。握手成功后在同一条连接提交一次业务请求；Host 拒绝未握手的业务调用。当前不推断不同应用版本之间的兼容性，不按二进制哈希要求相同构建；同版本开发构建需自行保持协议一致，正式发行修改不兼容协议时必须调整版本。

旧 Host 不认识握手方法，或任一版本不匹配、返回身份不完整、握手超时，CLI 都会在发送业务请求前返回错误，并提示检查、退出旧 Host。新 Host 的 `popu host status` 在 `host` 字段提供 `app_version`、`rpc_version`、`pid` 和 `executable`。升级后的恢复命令：

```powershell
python -m popu host status
# 确认可以结束当前 Host 后，也可从托盘退出：
python -m popu host stop
```

`host status` 和用户显式执行的 `host stop` 保留无需握手的管理通道，可管理仍使用 RPC 1.0 的旧 Host。不支持握手的旧 CLI 需要更新后才能向新 Host 提交业务请求。程序不会因为升级自动终止或替换已有 Host。

只有连接报告 Host 不存在或拒绝连接时才尝试启动。启动后连接就绪的等待窗口为 3 秒，兼容性握手另有 2 秒截止时间；`request.wait` 的业务等待不受握手时限影响。握手错误、权限错误和业务通信错误均不触发自动重放，防止响应丢失后重复创建请求。

Windows 上，在 CLI 启动后台 Host 之前以及显式运行 `host run` 之前，通过 Tauri 的 WebView2 loader 检查运行时是否可用。缺失或 loader 检测失败时，stderr 包含原始诊断、手动安装/修复提示及 [Microsoft 安装入口](https://developer.microsoft.com/microsoft-edge/webview2/)，退出码为 12；SDK 调用将其作为 `PopuError` 返回。检查不下载、不安装、不打开浏览器。已连接兼容 Host 时不需要另行启动或检查本地 WebView2。

检测使用 WebView2 官方 loader，而不是仅检查注册表，遵循其用户范围与运行时查找规则。参考 [Microsoft 运行时检测说明](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)。`--version`、模板查询和 Host 管理命令不依赖本次 CLI 能否创建 WebView。

第二阶段的审核重点是兼容/旧版/异常 Host、握手半帧超时、业务响应丢失、管理通道恢复，以及真实 loader 缺失场景。原生窗口回归使用独立 Named Pipe 和内存历史库；已有旧 Host 只做状态查询及握手拒绝验证，不停止它。发布 workflow、Trusted Publisher 和干净 Windows 系统的分发依赖验证仍留在第三阶段。

本次验证结果：完整 Rust 常规测试通过；最终代码再次通过 88 项库测试和 5 项 CLI 测试（另有 3 个子进程辅助测试按设计忽略），66 项 Python 测试通过。真实原生窗口回归在新握手连接下通过，包括通知/对话框生命周期、内容更新、等待、并发请求与 Host 身份诊断。重建后的 `dist/popu-0.1.0-py3-none-win_amd64.whl` 通过隔离 venv 安装、重装及缓存校验；安装后的 CLI 两种入口均验证了真实 loader 缺失提示，默认 SDK 验证了旧 Host 拒绝及恢复说明。
