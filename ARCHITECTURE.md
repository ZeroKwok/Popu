# Popu Host 架构

[English](ARCHITECTURE.en.md)

Popu 使用单实例常驻 Tauri Host，处理范围有限的本机人机交互。CLI 和 Python 进程都是客户端，不负责通知生命周期，也不单独创建 WebView 进程。

仓库由根目录的 just 任务统一编排。`ui/` 是独立的 npm 项目：静态源码位于 `ui/src/`，图表打包入口位于 `ui/charts/`，生成的分发资源位于 `ui/dist/`。前端依赖和浏览器测试不进入嵌入资源。`src-tauri/` 下的 Cargo 项目只使用 UI 构建产物；`python/` 项目打包 SDK 和准备好的原生程序。根目录 `scripts/` 保存跨项目构建与原生验证工具。

`src-tauri/Cargo.toml` 中的 `package.version` 是产品发行版本的唯一来源。Tauri 自动继承该版本，Python 构建和 editable 安装会在打包前生成被 Git 忽略的 SDK `_version.py`。安装后的 wheel 已包含此模块，不会读取仓库中的 Cargo 清单。协议版本与 RPC 版本独立用于兼容性识别。

```text
CLI / Python SDK
       │ 4-byte big-endian length + UTF-8 JSON RPC
       ▼
interprocess::local_socket
       │ Windows Named Pipe / Unix Domain Socket
       ▼
Tauri Host ── RequestRegistry ── request_id → WebviewWindow
                 ├─ one active dialog + global FIFO
                 └─ three visible notifications + global FIFO
```

## 生命周期与调度

- 首个客户端在后台启动 `popu host run`，并重试等待就绪。监听入口的绑定决定单实例边界。
- Windows 上，`host_process.rs` 通过 `CreateProcessW(..., bInheritHandles = FALSE, ...)` 分离启动 Host。Host 不继承提交方 CLI 的标准流或其他可继承句柄；仅重定向标准流不足以实现这种隔离。Unix 路径通过 `Command` 将标准流连接到空设备。
- Host 生成 UUID 请求 ID，每个请求的初始 revision 为 1。
- 请求状态包括 `queued`、`preparing`、`ready`、`shown`、`hidden`、`confirmed`、`cancelled`、`closed`、`expired`、`failed` 和 `stale`。`hidden` 不是终态，仍保留更新通道。
- 对话框按全局 FIFO 排队，同一时刻只有一个活动窗口。提交对话框的 CLI 等待终态响应，并采用相应动作的退出码。
- 通知显示在主显示器上，不受对话框队列阻塞，最多同时展示三条。提交客户端退出后，通知仍继续存在。`duration_ms` 从 `ui_ready` 开始计时。
- 通知窗口始终置顶，不主动请求焦点。省略 `window.always_on_top` 时默认为 `true`；显式设为 `false` 会被请求校验和 JSON Schema 拒绝。对话框仍可设置为 `false`。
- 可见请求关闭或展示超时后，队列中的后续请求会进入展示，通知栈也会重新排布。
- Host 退出时，普通请求进入终态并唤醒等待者。可更新通知保留持久化检查点，在下次启动时恢复为隐藏状态，同时保留无更新截止时间和用户关闭意图。

## 窗口尺寸与 DPI

请求中的 `window.width` 和 `window.height`，包括对话框与通知的默认尺寸，都使用逻辑像素。客户端在 1080p、2K 和 4K 显示器上应使用相同的值，不要按分辨率或 Windows 缩放比例手工放大尺寸。

默认宽度为通知 `360`、dialog `440`。省略高度时，首次排版按实际宽度测量整个窗口，在通知 `120–480`、dialog `160–720` 范围内自适应，并受目标区域减去上下各 16 边距的限制。首次显示后尺寸锁定，内容更新只改变内部布局，超长正文滚动。图片在解码前预留固定高度（默认 320），图表固定高度默认 220；显式窗口高度继续按原规则解析。未适配测量的外部模板使用受限的回退高度（通知 220、dialog 300）。实现边界见[首次自适应高度](docs/window-sizing.md)。

Windows 窗口排布使用两套明确分离的坐标空间：

```text
request logical size + logical margin
             │
             ▼
protocol / placement ── target monitor effective DPI ──► physical size + physical position
             │                                                  │
             ▼                                                  ▼
WebView logical CSS viewport                         Win32 work area / set_position
```

- `runtime.rs` 选择目标显示器，读取其有效 DPI，并获取以物理像素表示的工作区。
- `placement.rs` 先按目标显示器的逻辑工作区限制请求尺寸，再利用缩放比例计算原生窗口的物理尺寸、间距和位置。
- `host.rs` 先将隐藏窗口移至目标显示器，设置 WebView 的逻辑尺寸，再应用计算得到的物理位置，防止高 DPI 显示器缩小 CSS 视口或裁切靠右排列的窗口。
- 通知栈偏移量由同一排布结果换算为物理像素，保证不同 DPI 下的视觉间距一致。通知离开栈时会触发重排；周期性计时和仅更新内容的操作不会覆盖用户拖动后的位置。

`"100%" × "100%"` 经转换后仍填满所选的物理显示区域。逻辑视口随 DPI 变化，这是全屏窗口的预期表现。

## 请求标识、合并与更新

`coalesce_key` 是可选的活动业务位置标识，按请求模式分别作用。重复创建会返回已有请求的 ID、状态和 revision，不修改内容。请求进入终态后立即释放该标识。

更新包含 `request_id`、`expected_revision` 和完整的 `content` 快照。成功后 revision 递增，并刷新通知发生时间。通知在创建时通过 `updatable: true` 开启更新，默认值为 `false`；最后一次更新将其设为 `false`。更新时省略此字段会保留当前值。每次成功的通知更新都会按配置的展示时长重新计时，可通过 `duration_ms` 覆盖该时长。排队、准备或暂存中的通知在实际展示时获得完整时长。无效更新、revision 冲突和重复的就绪确认不会延长计时。对话框更新继续保留表单值和原始交互截止时间，并拒绝通知专用选项。

自动展示超时会收起可更新通知，下次上报会重新排队。用户主动关闭也会收起通知，但后续普通上报保持静默。显式设置 `reshow: true` 默认允许在主动关闭后再次展示一次，只在实际展示时消耗次数。关闭更新通道与展示最终内容相互独立。连续 12 小时没有成功上报时，请求以 `stale` 结束。完整规则与示例见[可更新通知](docs/live-notifications.md)。

外部模板属于调用方配置，通过 `--template` 提供。Host 在注册请求前解析模板目录的真实路径并进行校验。

内容接受与画面呈现分别记录。Host 只在已展示窗口收到当前版本的 `ui_ready`，或就绪窗口实际揭示时，推进内部已呈现版本；该版本不从检查点恢复。UI 关闭事件携带操作时的画面版本，原生关闭事件在接收时捕获 Host 已呈现版本，避免延迟关闭确认后来的内容。dialog 动作必须对应最新且已呈现的内容版本，冲突时保留窗口与输入，提示用户重新审阅；内容准备期间禁用动作按钮。无动作的关闭或 Esc 取消仍可结束 dialog。

表单统一通过 Popu 动作提交，拦截浏览器默认提交与页面导航。文本或数字输入框按 Enter 时，只有恰好一个 `submit: true` 动作才会提交，并校验全部表单块；多个提交动作必须点击选择。文本域 Enter 保留换行。通知不支持 Esc 收起，dialog 的 Esc 表示取消。

## RPC

每帧上限为 1 MiB。RPC 版本 `1.0` 提供以下方法：

- `request.create`、`request.update`、`request.dismiss`、`request.get`、`request.wait`
- `host.hello`
- `host.status`、`host.stop`
- `history.list`、`history.get`、`history.clear`

JSON 格式错误、版本不支持、未知方法、帧过大、revision 冲突、更新已终止的请求和 ID 不存在等情况都会返回结构化错误。

目前 `persistence_failed` 不保证操作未执行：Registry 或窗口可能已改变，后台还会重试保存。已知 ID 时先通过 `status` 核对 revision 和内容；创建未获得 ID 时，如原请求使用了 `coalesce_key`，重复创建只能复用仍活动的请求。没有该键或原请求已终止时，不保证重试幂等，不应自动盲目重放。

每条业务连接先发送 `host.hello`（携带 `params.app_version`）。Host 校验 CLI 的应用版本与帧中的 RPC 版本，然后返回 `app_version`、`rpc_version`、`pid` 和 `executable`。客户端验证身份后，在同一连接提交一次业务请求。应用版本和 RPC 版本都必须完全一致。握手使用非阻塞 I/O，在两秒截止时间内处理包括半帧在内的数据，不限制后续业务等待。Host 拒绝未经握手的业务调用。`host.status` 和显式调用的 `host.stop` 无需握手，便于升级恢复；状态响应中的 `host` 字段包含当前运行身份。

只有连接不存在或被拒绝时，客户端才自动启动 Host。握手失败和业务传输错误不会触发请求重放，也不会替换已有 Host。Windows 上，在分离启动 Host 或直接执行 `host run` 之前，通过 WebView2 loader 检查运行时可用性；失败时提示手动安装入口，不启动安装程序。Host 与 Python 分发的相关操作见 [Python 包构建与发布](docs/python-distribution.md)。

## 通知历史

通知历史和活动通知检查点存储在 Popu 应用数据目录下的 SQLite 数据库中。成功更新会替换已有历史记录，使内容、revision、发生时间和阅读状态保持一致。检查点保留请求、模板位置、用户主动关闭意图和再次展示次数。历史与检查点的变更在成功响应前原子提交。对话框及其表单值不会持久化。保留策略保存最新 500 条归档记录，并保留活动通知。

上述 500 条是数据库归档保留上限，不是 Host 内存中的请求数量上限；当前尚无终态 Registry 自动回收策略。`status` / `wait` 从当前 Host 的 Registry 读取，历史窗口与 `history.*` 从 SQLite 读取。Host 重启只恢复尚存的活动通知检查点，不重新装载已归档 ID 或 dialog，因此“历史仍可查看”不代表旧 ID 仍可通过 `status` / `wait` 查询。清除隐藏历史会删除对应更新通道，后续查询或更新返回 ID 不存在。

历史窗口以只读的主从布局展示当前和已归档通知快照。列表显示标题、展示状态和发生时间，详情区域渲染保存的正文。查看历史不会启动计时、改变通知栈或触发业务动作。清空历史会删除隐藏通知的更新通道，同时保留 `queued`、`preparing`、`ready` 和 `shown` 状态的通知。协调器对清空与更新操作进行串行处理。

通知快照保存成功或历史清空后，Host 向历史窗口定向发送 `history-changed` 事件。历史窗口合并刷新提示，并尽量保留当前选中项。窗口不会轮询数据库；未打开的历史窗口也不会收到刷新工作。

阅读状态按内容 revision 记录。用户关闭会确认已展示的 revision，后续内容重新进入未读状态。前台历史渲染器只确认实际渲染的 revision；迟到的确认不会将更新后的结果标为已读。托盘提醒仅取决于隐藏或归档记录中 `requires_read: true` 的未读项，不取决于可见或排队通知数量。

快照中的 `acknowledged_revision` 由 Host 内部维护，表示关闭或调用方撤销所确认的版本，不是请求输入字段。历史窗口另行记录带 revision 校验的阅读状态。已排队但未终态的一次性通知通常不会出现在历史列表中；持续更新会重置可见通知的计时，当前不承诺排队消息的最大等待时长。

## 公开 CLI

```text
popu request <file|-> [--template <trusted-directory>]
popu update <request-id> <update.json>
popu dismiss <request-id>
popu status <request-id>
popu wait <request-id>
popu host status
popu host stop
```

内部命令 `popu host run` 不在帮助中显示。退出码分别为：0 表示成功或确认，10 表示取消或关闭，11 表示超时，12 表示校验或运行错误。动作可以将对话框客户端的退出码覆盖为 0–255，不会终止 Host。

## 源码职责边界

- `core.rs`：不依赖 Tauri 的注册表、状态、FIFO 调度、合并、revision 校验与计时。
- `ipc.rs`：有界帧处理以及本地套接字客户端和监听器的构造。
- `ipc/handshake.rs`：Host 身份协商与业务连接握手。
- `host_client.rs`：连接获取与启动，不重放业务请求。
- `preflight.rs`：Host 启动前的 WebView2 可用性诊断。
- `host.rs`：RPC 分发、同步、多窗口管理与 UI 命令。
- `history.rs`：仅通知的 SQLite 持久化与保留策略。
- `runtime.rs`：目标显示器与 DPI 查询、物理位置应用及无边框窗口辅助操作。
- `placement.rs`：独立于 DPI 的逻辑尺寸计算，以及逻辑坐标到物理坐标的转换。
- `protocol.rs`：版本化请求与响应校验，以及逻辑窗口尺寸语义。

Python SDK 调用公开 CLI，提供 create、update、dismiss、status 和 wait，不实现第二套服务端或用于维持通知生命周期的管道。

SDK 要求 Python 3.10+ 和当前 Host CLI。接受回执采用严格解析，不兼容引入 Host 之前的响应格式。共用的 CLI 调用封装负责 stdout 管道、stderr 临时文件和进程清理。通知与管理调用先读取一行 UTF-8 JSON，再等待 CLI 退出，不等待 stdout 尾部 EOF，也不使用 stdout 暂存文件或轮询回执。阻塞读取由工作线程执行；调用线程使用有界队列等待和进程等待，使 Windows 能分发 Ctrl+C，并支持可选的单次操作截止时间。中断或超时时，先结束并回收 CLI，再等待读取线程并关闭流。Windows SDK 启动的 CLI 不带控制台，原生 Host 启动器负责隔离继承句柄。对话框被接受后，清除前台提交截止时间，由后台读取器以有界块排空剩余 stdout 并回收 CLI。每次 SDK `wait` 都调用公开 `wait` 命令并返回完整 Host 快照，因此结果不依赖提交请求的 Python 客户端。业务动作退出码（包括 12）从快照中读取，不解释为传输错误。临时请求和更新文件会在使用、中断或提交失败后移除。

Python 通知构造器显式设置主显示器工作区内的置顶窗口；对话框构造器保留置顶、显示器、区域和尺寸选项。两类构造器均提供内容对齐和活动合并键。表单、任意严重级别和自定义动作仍可通过完整请求字典提交。
