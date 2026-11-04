# Python 分发与阶段审核

目标是通过一个 `popu` Windows wheel 安装原生工具和 Python SDK。三个阶段的代码已实现，第三阶段等待审核及 GitHub 首次运行，尚未向 PyPI 发布。

## 阶段划分

| 阶段 | 范围 | 当前状态 |
| --- | --- | --- |
| 1 | 本地 wheel、命令入口、SDK 默认解析、不可变运行时缓存与安装验证 | 已审核 |
| 2 | CLI 与已有 Host 的版本/协议握手，WebView2 缺失诊断和手动安装入口 | 已审核 |
| 3 | 发布 workflow、发行版本检查、制品验证和 PyPI Trusted Publisher | 已实现，等待审核 |

每阶段完成后停下审核。`.github/workflows/python-publish.yml` 已接通构建、验证和 Trusted Publishing；先通过手动运行审核制品，再发布 GitHub Release。

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
python -m pip install build twine packaging pytest
just build-wheel
python -m twine check --strict ./dist/popu-0.1.1-py3-none-win_amd64.whl
python scripts/verify-python-release.py --wheel ./dist/popu-0.1.1-py3-none-win_amd64.whl --smoke --report test-results/wheel-review.json

python -m venv test-results/popu-review
./test-results/popu-review/Scripts/python.exe -m pip install --no-index --no-deps ./dist/popu-0.1.1-py3-none-win_amd64.whl
./test-results/popu-review/Scripts/popu.exe --version
./test-results/popu-review/Scripts/python.exe -m popu templates list
./test-results/popu-review/Scripts/python.exe -m popu.demo progress --wait
```

若需额外依赖库，可在 staging 时重复传入 `--dll <path>`；必须为 x64 PE DLL，并通过 VC 运行库依赖检查。staging 输出位于被 Git 忽略的 `python/src/popu/_native/`，wheel 输出位于 `dist/`。未准备或损坏的原生载荷会阻止 wheel 构建。源码 editable 安装允许不含载荷，但这时必须显式指定本地可执行文件。

`just build-wheel` 依次构建 `ui/dist/`、静态 CRT 的原生 release 程序、Rust 依赖许可证、版本化载荷和 wheel。UI 的依赖、脚本和测试不会进入 Tauri 嵌入资源。只修改 Cargo 中的版本后，需重新编译原生程序；载荷准备不会自动拿旧 exe 冒充新版本。SDK 安装后使用随包生成的版本文件，不读取仓库中的 Cargo 清单。

原生构建会先按需准备图标。wheel 专用原生程序位于 Cargo target 目录下的 `x86_64-pc-windows-msvc/release/popu.exe`；普通 `just build-release` 仍是开发构建，不能直接作为便携 wheel 载荷。工具通过 Cargo metadata 尊重 `CARGO_TARGET_DIR`。若只更新载荷，可在许可证已生成后，运行 `python scripts/stage-python-runtime.py --executable <静态CRT程序>`；默认也选择 wheel 专用程序。当前 wheel 配方及载荷准备仅支持 Windows；Python 解释器可通过 `PYTHON` 环境变量指定。

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

## 第三阶段：发行与发布

宏观检查确认版本已统一为 Cargo 单源，UI 产物与源码分离，CLI/Host 握手与不可变缓存已经接通。发现旧 release exe 依赖 `VCRUNTIME140.dll` 和 `VCRUNTIME140_1.dll`；wheel 构建现使用显式 Windows x64 target 和 `+crt-static`，避免额外安装 VC Redistributable。staging 和制品校验都会检查 PE 普通导入与延迟导入，拒绝这些依赖。Windows 自带 DLL 和用户手动安装的 WebView2 仍是系统前提。

原生依赖通知由固定版本 `cargo-about 0.9.3` 生成，按 `src-tauri/about.toml` 选择许可文本；无法解析的依赖会阻止构建。`just build-wheel` 会复用已有匹配版本，或自动安装到被 Git 忽略的 `python/build/tools/cargo-about-0.9.3/`；直接调用该缓存里的程序，不依赖工具目录位于 PATH，也不替换全局安装。首次工具安装需要联网并编译数分钟。wheel 同时保留项目许可证、UI 依赖许可证和 Rust 依赖许可证，后者包含未修改依赖的精确版本源码下载地址，并绑定当前 Cargo.lock 的 SHA-256。`src-tauri/about.hbs` 是生成模板。构建许可证工具需要联网获取未随 crate 打包的上游许可证。

工作流的三个 job：

1. `release-build` 在 Windows Server 2022 上准备 Python、Node、Rust、just、ImageMagick 和许可证工具，运行 `just check`，构建唯一的 Windows wheel，并进行 twine 与制品内容检查。
2. `installed-wheel` 在新的 Windows runner 上，分别使用 Python 3.10 和 3.13 检查同一 wheel。脚本创建隔离 venv、清除源码导入路径和构建工具 PATH，验证 SDK、两种 CLI 入口、内置演示帮助、UTF-8 输入、退出码、WebView2 缺失提示、缓存定位、重装与卸载后的缓存保留。它不会启动可见窗口或连接用户业务 Host。
3. `pypi-publish` 仅由 GitHub Release 的 `published` 事件触发，等待所有验证成功后，在 Ubuntu 上发布下载到的原制品。只发布 wheel，不发布 sdist；使用 OIDC Trusted Publishing 和制品证明，不需要 PyPI API token。

手动 `workflow_dispatch` 只构建和验证，上传 `release-dists`、构建报告和各 Python 安装报告，发布 job 会跳过。工作流文件名必须保持 `python-publish.yml`，环境名保持 `pypi`。

首次发布前核对 Pending Publisher：

| 字段 | 值 |
| --- | --- |
| PyPI project | `popu` |
| GitHub owner | `ZeroKwok` |
| Repository | `Popu` |
| Workflow filename | `python-publish.yml` |
| Environment | `pypi` |

这些值必须与 PyPI 页面中创建的 Pending Publisher 一致。首次成功上传会创建项目；Pending Publisher 不保留项目名称。参考 [PyPI Pending Publisher 文档](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/) 和 [发布动作说明](https://github.com/pypa/gh-action-pypi-publish)。

发布步骤：提交并推送已审核代码 → 手动运行本 workflow 并检查 artifacts → 创建标签 `v<Cargo版本>` → 发布该标签的 GitHub Release。以当前 Cargo 版本为例，标签必须为 `v0.1.1`；不一致会在原生编译前失败。修改版本后先用常规 `just build` 或 `just build-release` 让 Cargo 更新生成的 Cargo.lock，并随代码提交；CI 使用 `--locked`，不会默默更新锁文件。SemVer 的 `-rc.1` 等兼容预发布写法由 Python 元数据规范化，原生版本保留 Cargo 写法；禁止向 PyPI 上传带 `+...` 本地构建元数据的版本。

首次发布成功后，用户可执行 `python -m pip install popu`，然后使用 `popu`、`python -m popu` 或 Python SDK。当前检查只验证本地构建与工作流结构，不代表已完成远端 OIDC 身份验证或实际 PyPI 上传；没有从本次开发会话触发发布。隔离 runner 也不能代替所有 Windows 10/11 系统环境的虚拟机验证。

本次本地验证：14 项 UI 测试、39 项构建工具测试、80 项 Python SDK 测试和完整 Rust 常规测试通过；另用静态 CRT release 构建运行真实原生窗口回归，通知、更新、等待、对话框及八轮混合布局均通过。`actionlint` 与 `twine check --strict` 通过。最终 0.1.1 wheel 在 Python 3.11.9 上通过隔离安装、运行、重装和卸载缓存保留验证，报告位于 `test-results/release-wheel-review.json`。Python 3.10/3.13 矩阵和 OIDC 发布等待 GitHub 首次运行验证。
