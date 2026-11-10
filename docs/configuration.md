# Host 配置

在 Windows 上，默认配置文件是 `%LOCALAPPDATA%\Popu\config.toml`。运行 `popu config` 即可检查文件，缺失时会写出[内置模板](../src-tauri/resources/config.template.toml)。模板注释说明了各项设置，取消注释即可覆盖对应默认值。以下是全部字段及内置默认值，尺寸和间距均为逻辑像素。

```toml
[notification]
edge_margin_px = 16
gap_px = 12
max_visible = 3
default_width_px = 360
auto_min_height_px = 120
auto_max_height_px = 480
fallback_height_px = 220
default_duration_ms = 8000

[dialog]
default_width_px = 440
auto_min_height_px = 160
auto_max_height_px = 720
fallback_height_px = 300
default_timeout_seconds = 300
```

- 通知按主显示器工作区右下角对齐，`edge_margin_px` 控制底部和右侧留白，`gap_px` 控制相邻通知的垂直间距。可同时显示的数量由 `max_visible` 和实际可用空间共同决定。
- 省略窗口宽度时使用 `default_width_px`。省略高度时按内容自适应，限制在 `auto_min_height_px` 和 `auto_max_height_px` 之间。测量不可用时采用 `fallback_height_px`，小工作区的实际空间优先。
- 请求显式提供的 `window.width`、`window.height`、`duration_ms` 或 `timeout_seconds` 优先。展示计时从窗口正式显示开始。通知更新省略 `duration_ms` 时沿用该通知的当前时长。

| 命令 | 行为 |
| --- | --- |
| `popu config` | 验证默认配置文件。缺失时先生成模板，报告中的 `initialized` 表示本次是否创建了文件。 |
| `popu config validate [PATH]` | 只读验证。省略路径时检查默认文件。缺失、不可读或校验失败时返回非零退出码。 |
| `popu config init [--force]` | 在默认路径生成模板。文件已存在时需要 `--force` 才会替换整份文件。 |
| `popu config path` | 打印默认配置路径，即使文件尚未创建。 |

配置命令可以在 Host 停止时运行。Host 只在启动时读取配置。修改后可先运行 `popu config validate`，然后退出托盘里的 Host 或运行 `popu host stop`。下一次请求会启动 Host 并读取新配置。

[JSON Schema](../protocol/config.schema.json) 校验字段名、类型和取值范围。补齐缺省值后，两组高度分别需要满足 `auto_min_height_px <= fallback_height_px <= auto_max_height_px`。缺少配置文件时使用内置默认值。配置不可读或校验失败时，Host 使用整套内置默认值，并在 `popu host status` 的 `config` 中报告路径和错误。
