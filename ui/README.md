# Popu UI

此目录是独立的 npm 项目，仅负责界面资源、图表 bundle 和前端测试。整个项目的构建、原生测试和 Python 打包由仓库根目录的 just 编排。

在本目录中可执行：

```powershell
npm ci
npm run build
npm run check
npm test
npm run test:visuals
```

- `src/`：HTML、CSS、界面 JavaScript 和内置模板。
- `src/window-sizing.js`：首次显示前的自然高度测量；高度边界及尺寸锁定由 Host 的 `WindowLayout` 管理，详见[尺寸约定](../docs/window-sizing.md)。
- `charts/`：交给 esbuild 打包的图表运行时与 worker 入口。
- `scripts/`：资源构建和浏览器测试服务器。
- `tests/`：JS 单元测试与 Playwright 回归；协议示例读取仓库共享的 `protocol/`。
- 浏览器回归按示例尺寸检查短内容横向和纵向均不溢出，并验证长内容仍能滚动至末尾；通知检查保留时间戳。
- `dist/`：生成的完整静态资源，包含离线图表库及第三方许可证。Tauri 和 Rust 内置模板只使用此目录。

`node_modules/`、构建脚本、测试和包元数据均不进入 `dist/`。npm 包不声明产品版本；产品版本只维护在 `src-tauri/Cargo.toml`。
