# Python 包构建与发布

本指南面向维护者，说明如何构建包含原生工具和 Python SDK 的 wheel、验证制品并发布到 PyPI。安装和使用见 [Python 入门](../python/README.md)，示例见 [demo 文档](../python/src/popu/demo/README.md)。

## 分发约定

- 分发名、导入名和命令名均为 `popu`，同时提供 `python -m popu` 入口。
- 目前仅分发 Windows x64 wheel，标签为 `py3-none-win_amd64`，要求 64 位 Python 3.10+。不发布 sdist，用户安装时无需编译。
- wheel 包含匹配版本的原生程序、Python SDK、demo、项目许可证、UI 与 Rust 依赖许可证，以及原生文件的 SHA-256 清单。
- 原生程序使用静态 CRT；构建校验拒绝额外的 VC Redistributable 依赖。WebView2 Runtime 仍需系统已安装，缺失时提供手动安装入口。
- SDK 默认使用包内程序，不搜索 `PATH`。运行时从不可变缓存启动，避免 Host 长驻时锁住 `site-packages`。升级和卸载不会自动停止已有 Host；缓存与版本握手规则见 [SDK 参考](python-sdk.md#运行时与源码开发)。

## 本地构建

构建机器目前需要 Windows x64、Python 3.11+、Rust stable（MSVC 工具链及其链接器）、Node.js、just 和 ImageMagick。构建环境可参考 [发布 workflow](../.github/workflows/python-publish.yml)；安装后的用户无需这些编译工具。

以下命令均在仓库根目录执行。先在开发虚拟环境中安装 Python 工具，再准备源码并构建：

```powershell
python -m pip install build twine packaging pytest
just setup
just build-wheel
```

`just build-wheel` 依次准备 UI 和图标，构建静态 CRT 的原生 release 程序，生成依赖许可证，准备原生载荷，最后构建 wheel。构建使用 `Cargo.lock`，不会自动更新依赖锁定结果。

| 输出 | 位置 |
| --- | --- |
| UI 构建资源 | `ui/dist/` |
| wheel 专用原生程序 | Cargo target 目录下的 `x86_64-pc-windows-msvc/release/popu.exe` |
| 原生依赖许可证 | `python/build/RUST_THIRD_PARTY_LICENSES.txt` |
| 生成的原生载荷 | `python/src/popu/_native/` |
| wheel | `dist/` |

Cargo target 目录通过 metadata 解析，支持 `CARGO_TARGET_DIR`；可用 `PYTHON` 环境变量指定 just 使用的解释器。`_native/` 是生成目录，不直接作为运行位置，也不提交 Git。

### 原生载荷与许可证

修改 CLI、Host、依赖或产品版本后，重新运行 `just build-wheel`，由工具同步重建程序、许可证和载荷。普通 `just build`、`just build-release` 使用开发构建配置；wheel 配方通过显式 target 和 `target-feature=+crt-static` 构建便携程序。

载荷准备工具会检查原生程序版本、x64 PE 格式、普通及延迟导入的 DLL、文件清单和校验值。wheel 构建会再次校验载荷，拒绝未准备、损坏或版本不匹配的文件。

如已有符合要求的原生程序和当前依赖许可证，可单独准备载荷：

```powershell
python scripts/stage-python-runtime.py
```

默认读取 wheel 专用程序路径，也可用 `--executable <路径>` 指定其他已构建的静态 CRT 程序。需要额外的 x64 DLL 时可重复传入 `--dll <路径>`；DLL 同样必须通过便携性检查。

Rust 许可证由构建脚本中固定版本的 `cargo-about` 生成。脚本复用匹配版本，或自动安装到 `python/build/tools/`，无需手动复制工具或修改全局 PATH。工具安装及上游许可证获取需要联网。许可证选择和模板分别位于 [about.toml](../src-tauri/about.toml) 与 [about.hbs](../src-tauri/about.hbs)，生成结果绑定当前 Cargo.lock 的 SHA-256。许可证缺失或过期会阻止构建。

## 制品验证

运行源码回归检查：

```powershell
just check
```

校验已有 wheel：

```powershell
just verify-wheel
```

该入口执行 `twine check --strict` 及制品内容校验，检查元数据、版本一致性、wheel 标签、命令入口、许可证、PE 依赖、载荷哈希和 RECORD，报告写入 `test-results/wheel-verify.json`。

测试隔离安装：

```powershell
just test-wheel
```

该入口包含上述校验，并创建临时虚拟环境，清除源码导入路径及构建工具 PATH，验证两种 CLI 入口、SDK、demo 帮助、UTF-8 输入、错误退出码、WebView2 缺失提示、缓存位置、重装和卸载后的缓存保留。结束后清理临时环境，报告写入 `test-results/wheel-test.json`。

这两个入口复用 [verify-python-release.py](../scripts/verify-python-release.py)。默认从 Cargo 版本计算 Python 规范化版本，并选择 `dist/popu-<版本>-py3-none-win_amd64.whl`。它们不会重新构建，也不会从其他版本中挑选文件；缺少对应制品时提示先运行 `just build-wheel`。

可指定已下载或放在其他目录的 wheel，以及自定义报告位置：

```powershell
just verify-wheel --wheel "path with spaces/popu-<版本>-py3-none-win_amd64.whl" --report test-results/candidate-verify.json
just test-wheel --wheel "path with spaces/popu-<版本>-py3-none-win_amd64.whl" --report test-results/candidate-test.json
```

示例路径需替换为实际 wheel 路径，保留原始制品文件名。校验会对照当前源码版本、项目许可证和 Cargo.lock；检查 CI 制品时应检出其对应提交。

### 安装到当前环境

在希望安装的 Python 环境中执行：

```powershell
just setup-wheel
```

该入口先校验制品内容，再使用配方选定的 Python 执行 `pip install --force-reinstall --no-index --no-deps`，将指定 wheel 安装到该解释器的当前环境。同版本也会重新安装；如果当前使用 editable SDK，会切换为 wheel 安装。

默认使用 `python`，可通过 `PYTHON` 环境变量指定解释器。安装前会打印目标解释器路径，安装后打印使用该解释器运行 demo 的命令。它不会创建额外环境、启动 demo、停止 Host 或重新构建制品。

也可以显式选择 wheel：

```powershell
just setup-wheel --wheel "path with spaces/popu-<版本>-py3-none-win_amd64.whl"
```

### 桌面验收

安装完成后，在同一个 Python 环境的真实终端中运行：

```powershell
popu --help
popu templates list
python -m popu --version
python -m popu.demo progress
```

若通过 `PYTHON` 选择了其他解释器，后续命令也应使用该解释器；可直接复制 `setup-wheel` 输出的 demo 命令。若 `popu` 不在 PATH，使用 `python -m popu` 代替。

自动安装验证不代替真实终端与桌面检查。确认帮助、模板列表及错误提示正常显示；在桌面确认进度更新、主动关闭、最终摘要和历史显示正常。可给 demo 加 `--wait` 等待最终通知结束展示，并检查 Ctrl+C 能及时停止演示。

若已有不同版本的 Host，先查看 `host status`，确认其中的交互可以结束后再从托盘退出或执行 `host stop`。随后启动 demo，确保验收使用新版本 Host。

## 版本更新

产品版本唯一来源是 [src-tauri/Cargo.toml](../src-tauri/Cargo.toml) 的 `package.version`。Tauri 自动继承此版本；Python 构建、editable 安装和载荷准备自动生成被 Git 忽略的 `_version.py`，无需维护第二处版本号。

修改版本后同步锁文件和本地 SDK 版本：

```powershell
cargo update --manifest-path src-tauri/Cargo.toml --offline --workspace
python python/build_version.py

$popuVersion = python python/build_version.py --print
$releaseTag = "v$popuVersion"
python scripts/verify-python-release.py --version-only --tag $releaseTag
```

检查 Cargo.lock 的差异并随源码提交。版本变化后需要重新构建原生程序和 wheel；载荷准备会拒绝使用旧 exe。

Release 标签必须精确等于 `v<Cargo版本>`。SemVer 预发布版本在 Python 元数据中规范化，标签仍使用 Cargo 原始版本。发布版本不可包含 `+...` 本地构建元数据；已上传的制品不能被覆盖，修正后应使用新版本。

`python/README.md` 用作 PyPI 项目介绍。修改它只会影响后续构建和上传，不会改变已发布版本的介绍页。

## 正式发布

发布由 [python-publish.yml](../.github/workflows/python-publish.yml) 编排：

| Job | 环境与职责 |
| --- | --- |
| `release-build` | 固定在 `windows-2022`，执行源码检查，构建静态 CRT wheel，校验元数据和内容 |
| `installed-wheel` | 使用 `windows-latest`，分别在 Python 3.10 和 3.13 下验证同一个 wheel 的隔离安装 |
| `pypi-publish` | 等待上述检查全部成功，在 Ubuntu 上通过 OIDC Trusted Publishing 上传原制品并生成制品证明 |

构建环境固定在较早的 Windows 镜像，以维持兼容性基线；安装验证跟随 GitHub 最新稳定 Windows 镜像。具体工具和 Actions 版本以 workflow 为准。

### 发布步骤

1. 提交版本更新及源码，推送到 GitHub，手动运行此 workflow（`workflow_dispatch`）验证待发布提交。
2. 检查构建和安装结果、制品报告，并完成目标 Windows 桌面的验收。
3. 在同一个已审核提交上创建 `v<Cargo版本>` 标签并推送至 GitHub，然后发布该标签的 GitHub Release。
4. Release 的 `published` 事件触发正式流程，重新构建、验证并上传 PyPI。检查 `pypi-publish` 成功，再核对 PyPI 上的版本和文件。

手动运行只构建和验证，发布 job 会跳过。可下载 `release-dists`、`wheel-build-report`、`installed-wheel-3.10` 和 `installed-wheel-3.13`，核对报告中的 wheel 哈希是否一致。正式 Release 会重新构建，应以其构建和安装报告确认最终上传制品。

发布后从 PyPI 安装并检查：

```powershell
python -m pip install --upgrade popu
python -m popu --version
python -m popu.demo progress
```

### Trusted Publisher 配置

PyPI 项目使用已有 Trusted Publisher，配置必须与实际发布身份一致：

| 字段 | 值 |
| --- | --- |
| PyPI project | `popu` |
| GitHub owner | `ZeroKwok` |
| Repository | `Popu` |
| Workflow filename | `python-publish.yml` |
| Environment | `pypi` |

发布 job 使用 `pypi` 环境并申请 `id-token: write`，无需 PyPI API token。仓库、所有者、workflow 文件名或环境名变化时，需要同步更新 PyPI 配置。若该 GitHub 环境配置了审核者，还需在发布时通过环境审核。

账户配置核对、桌面验收和发布决定由维护者完成。构建、许可证生成、制品校验、安装矩阵及上传由 workflow 自动执行。

## 故障处理

| 现象 | 处理 |
| --- | --- |
| 载荷版本不一致、缺少文件或校验失败 | 从当前源码重新执行 `just build-wheel`，不要手动混入旧程序或修改清单 |
| 程序依赖 VC Redistributable | 使用 wheel 配方生成静态 CRT 程序，检查是否误用了普通 release 构建或外部 DLL |
| 原生许可证缺失或过期 | 重新执行完整 wheel 构建，检查工具安装和上游许可证获取日志 |
| 安装后找不到 `popu` 命令 | 使用 `python -m popu`，并检查安装时使用的 Python 环境与 scripts 目录 PATH |
| WebView2 不可用 | 按诊断从 [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/) 手动安装或修复运行时 |
| 升级后提示 Host 不兼容 | 查看 `python -m popu host status`，确认后退出旧 Host，再重新请求 |
| PyPI 上传失败 | 查看发布 job 日志，核对 Trusted Publisher 身份、环境审核及 PyPI 实际上传状态，再决定后续操作 |

运行时缓存、Host 握手、超时与错误行为的细节见 [SDK 参考](python-sdk.md)。
