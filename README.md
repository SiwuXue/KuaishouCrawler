# 快手采集工作台

基于 Tauri 2 + Vue 3 的本地桌面采集工作台，包含任务配置、实时运行日志、真实数据文件浏览/预览/导出和本机运行环境设置。采集引擎源码随项目放在 `backend/engine`；开发环境运行时安装在 `backend/.runtime`，安装版运行时安装在应用自己的本地数据目录。应用运行不需要访问其他项目目录。

## 安装本机采集环境

在 Windows PowerShell 中，从项目目录执行：

```powershell
npm run backend:setup
```

安装脚本会在项目目录中准备独立的 Python 3.11、引擎依赖和 Chromium，并使用随项目提供的 Node.js 与 uv，不使用系统 Python/Node 环境。应用设置页也可以启动此安装流程。

## 启动应用

```powershell
npm install
npm run tauri dev
```

`npm run build` 会编译 Vue 前端；`npm run tauri build` 会生成 Windows 安装包并附带采集引擎、安装工具和脚本。Python、依赖与 Chromium 首次启动时通过设置页安装到应用自己的本地数据目录，运行时不依赖其他项目目录。

## 数据与合规

任务和设置保存在本机。默认数据目录 `data/ks/` 实际写入应用本地数据目录，可在设置页改为其他文件夹。采集引擎来源及其上游用途说明见 [UPSTREAM_NOTICE.md](backend/UPSTREAM_NOTICE.md) 和 `backend/engine/README.md`。
