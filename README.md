<div align="center">

# 抖音直播录制工具 · 弹幕版

**桌面与无头 Web 双形态的抖音直播录制工具**

监控直播间 | 自动/定时录制 | 弹幕录制（分段对齐） | ASS 字幕导出与烧录

[![CI](https://github.com/ruanun/douyin-recorder/actions/workflows/ci.yml/badge.svg)](https://github.com/ruanun/douyin-recorder/actions/workflows/ci.yml)
[![Release](https://github.com/ruanun/douyin-recorder/actions/workflows/release.yml/badge.svg)](https://github.com/ruanun/douyin-recorder/actions/workflows/release.yml)
![Tauri 2](https://img.shields.io/badge/Tauri-2.x-blue?logo=tauri)
![Vue 3](https://img.shields.io/badge/Vue-3.x-brightgreen?logo=vuedotjs)
![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![Docker](https://img.shields.io/badge/Docker-ready-2496ED?logo=docker)
![License](https://img.shields.io/badge/License-MIT-yellow)

</div>

---

## 简介

本项目基于 [SplashedWhite/douyin-recorder](https://github.com/SplashedWhite/douyin-recorder)（MIT），
在保留原有桌面体验的基础上，补齐了**弹幕**相关的完整链路，并增加了一个**无图形界面的 Web 服务端**：

| 能力 | 上游 | 本项目（fork） |
|------|------|----------------|
| 直播监控 / 自动录制 / 分段 / 断流恢复 | ✅ | ✅（保持兼容） |
| **弹幕录制**（WebSocket + protobuf） | ❌ | ✅ XML，B 站兼容格式 |
| **弹幕与视频分段一一对齐** | ❌ | ✅ `part001.flv` ↔ `part001.xml` |
| **ASS 字幕**（自动导出 / 手动上传转换 / 烧录进视频） | ❌ | ✅ |
| **无头 Web 服务端**（浏览器操作，零 GTK/WebKit 依赖） | ❌ | ✅ 单二进制 / Docker |
| 桌面端 | ✅ | ✅ 打包 deb/rpm/AppImage/NSIS/MSI |

> 技术架构：`core`（业务核心，无 UI 依赖）+ `src-tauri`（桌面壳）+ `server`（Web 服务）。
> 两种形态共享同一套业务逻辑与同一份前端页面。

---

## ✨ 功能特性

### 录制

- **直播间监控** —— 添加 `live.douyin.com` 直播间链接，实时查看开播状态
- **手动 / 自动 / 定时录制** —— 逐房间开启开播检测，支持每日定时开启监控窗口与持续监控
- **多画质** —— 原画 / 蓝光 / 超清 / 高清 / 标清（新安装默认原画）
- **分段录制** —— 按分钟切分，每段独立成文件与记录
- **断流恢复** —— 推流中断后在限定时间内自动重连续录，生成接续任务
- **FLV → MP4** —— 录制完成可自动或手动无损转封装
- **按主播分文件夹** —— 昵称变化后继续使用原文件夹
- **代理 / Cookie** —— 适配不同网络与登录态

### 弹幕

- **实时录制** —— 通过抖音 Webcast WebSocket 协议记录弹幕（含昵称、颜色、表情文本）
- **B 站兼容 XML** —— 时间轴与录制起点对齐，可直接被弹弹play 等播放器加载
- **分段对齐** —— 每段视频对应同名 XML，切换分段时弹幕文件同步轮换且连接不断开
- **自动 ASS 字幕** —— 录制结束自动生成同名 `.ass`（可在设置中关闭）
- **手动转换工具** —— 上传任意 XML 弹幕文件，一键转 ASS 并下载/保存
- **烧录字幕** —— 用 FFmpeg + libass 生成带弹幕的 MP4

### 部署

- **桌面版** —— Tauri 2，轻量、托盘常驻、开机可用
- **Web 服务端** —— 单个可执行文件内嵌前端，浏览器操作；`ldd` 不依赖 GTK/WebKit
- **Docker** —— 多阶段构建，非 root 运行，`./data` 即全部数据

---

## 📋 截图

<div align="center">

> 主界面：直播间列表 + 录制任务（含弹幕徽标与字幕入口）
>
> ![screenshot](assets/screenshot4.png)
>
> ![screenshot](assets/screenshot3.png)

</div>

---

## 📥 桌面版安装

1. 从 Releases 下载对应安装包：
   - Linux：`.deb` / `.rpm` / `.AppImage`
   - Windows：`.exe`（NSIS）/ `.msi`
2. 安装后运行 `douyin-recorder`，粘贴直播间链接即可。
3. 首次使用建议在「设置」里确认录制目录与是否开启「弹幕录制 / 自动生成 ASS 字幕」。

> **FFmpeg**：安装包已内置静态 FFmpeg（sidecar），无需另行安装。

---

## 🐳 Web 服务端部署（推荐用于服务器/NAS）

服务端没有图形界面，通过浏览器使用，功能与桌面版一致。

### 方式一：Docker Compose（最简单）

```bash
git clone <本仓库>
cd douyin-recorder
cp .env.example .env          # 修改 DOUYIN_RECORDER_TOKEN
docker compose up -d
```

打开 `http://<主机>:8787/`，首次调用接口会提示输入令牌。

```bash
docker compose logs -f        # 查看日志
docker compose down           # 停止（数据保留在 ./data）
```

### 方式二：直接运行 GHCR 镜像

```bash
docker run -d --name douyin-recorder --restart unless-stopped \
  -p 8787:8787 \
  -e DOUYIN_RECORDER_TOKEN=$(openssl rand -hex 16) \
  -e PUID=$(id -u) -e PGID=$(id -g) \
  -v "$PWD/data:/data" \
  ghcr.io/ruanun/douyin-recorder:latest      # 发布工作流推送的镜像
```

也可以把它写进 `.env` 复用 compose：

```bash
echo 'DOUYIN_RECORDER_IMAGE=ghcr.io/ruanun/douyin-recorder:latest' >> .env
docker compose pull && docker compose up -d --no-build
```

### 方式三：二进制 + systemd

```bash
# 从 Releases 下载 douyin-recorder-server-<version>-linux-x86_64.tar.gz
sudo useradd -r -m -d /var/lib/douyin-recorder douyin-recorder
sudo cp douyin-recorder-server /usr/local/bin/
sudo cp douyin-recorder.service /etc/systemd/system/
echo 'DOUYIN_RECORDER_TOKEN='$(openssl rand -hex 16) | sudo tee /etc/douyin-recorder.env
sudo chmod 600 /etc/douyin-recorder.env
sudo chown -R douyin-recorder: /var/lib/douyin-recorder
sudo systemctl enable --now douyin-recorder
```

### 环境变量

| 变量 | 说明 | 默认 |
|------|------|------|
| `DOUYIN_RECORDER_HOME` | 配置、数据库、日志与默认录制目录 | `~/.douyin-recorder` |
| `DOUYIN_RECORDER_BIND` | 监听地址 | `0.0.0.0:8787` |
| `DOUYIN_RECORDER_TOKEN` | 访问令牌（`X-Auth-Token` 头或 `?token=`） | 不启用认证（会打印警告） |
| `PUID` / `PGID` | 容器运行身份（与宿主机用户一致，文件属主才属于你） | `1000` |
| `TZ` | 时区 | `Asia/Shanghai` |

### HTTP 接口

| 接口 | 说明 |
|------|------|
| `POST /api/invoke/{command}` | 与桌面版 `invoke` 同名同参（`get_rooms`、`start_record`、`save_settings_cmd` …） |
| `GET /api/events` | Server-Sent Events，事件名与桌面版一致 |
| `POST /api/invoke/convert_danmaku_xml` | 上传 XML 文本转 ASS：`{"content": "..."}` → `{ass, count}` |
| `POST /api/invoke/export_danmaku_ass` | 为已有录制生成 ASS：`{"taskId"}` 或 `{"segmentId"}` |
| `POST /api/invoke/burn_danmaku` | 烧录字幕并输出 `<原名>_danmaku.mp4` |
| `GET /api/health` | 健康检查（无需认证） |

> 安全建议见 [SECURITY.md](SECURITY.md)：请务必设置强令牌，并尽量置于内网或 HTTPS 反向代理之后。

---

## 弹幕与字幕说明

录制完成后目录结构（分段示例）：

```
主播名_房间号_时间_任务号_part001.flv    # 视频
主播名_房间号_时间_任务号_part001.xml    # 弹幕（B 站兼容）
主播名_房间号_时间_任务号_part001.ass    # 字幕（自动生成）
```

- **自动生成**：设置 →「弹幕录制」→「自动生成 ASS 字幕」（默认开启）
- **手动转换**：页面右上角上传图标，选择任意 XML 弹幕文件 → 转换为 ASS（保存到录制目录）
- **烧录**：任务列表 → 弹幕字幕图标 → 烧录到视频（需 FFmpeg 带 libass；Docker 镜像已内置中文字体）
- **文件获取**：Web 模式不提供浏览器下载，录像/弹幕/字幕直接保存在服务器的
  `data/recordings`（宿主机的 `./data/recordings`）中，可通过文件共享、SFTP 或 `docker cp` 取用

ASS 转换规则参考 [danmaku2ass](https://github.com/m13253/danmaku2ass) 与
[danmu2ass](https://github.com/gwy15/danmu2ass)：滚动弹幕 `\move`、顶/底部 `\pos`、
按长度分配轨道避免重叠、颜色按 ASS 的 `BBGGRR` 写入。

---

## 发布产物

打 `v*` 标签后，`.github/workflows/release.yml` 会产出：

| 产物 | 内容 |
|------|------|
| Linux 桌面 | `.deb` / `.rpm` / `.AppImage`（已内嵌 FFmpeg） |
| Windows 桌面 | NSIS `.exe` / `.msi`（已内嵌 FFmpeg） |
| 服务端 | `douyin-recorder-server-<version>-linux-x86_64.tar.gz`（**单二进制已内嵌 Web 前端**）+ systemd 单元 + `.env.example` + sha256 |
| 校验和 | `SHA256SUMS.txt`（覆盖全部发布文件） |
| 容器镜像 | `ghcr.io/ruanun/douyin-recorder:latest` 与 `:<version>` |

> 服务端无需另配前端文件：`server/build.rs` 会把 `dist/` 编译进二进制。macOS 桌面版请参考开发指南本地构建（未纳入自动发布）。

## 开发指南

### 环境要求

- Node.js ≥ 18、pnpm 10
- Rust stable（rustup）
- `protoc`（protobuf-compiler，用于生成弹幕协议代码）
- Linux 桌面打包需要：`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf`
- FFmpeg：桌面打包需放入 sidecar（见下），服务端可使用系统 FFmpeg

### 目录结构

```
douyin-recorder/
├── Cargo.toml               # workspace: core / src-tauri / server
├── core/                    # 业务核心（无 Tauri/axum 依赖）
│   ├── src/parser.rs        #   抖音直播间解析（enter API、画质选择）
│   ├── src/recorder.rs      #   FFmpeg 进程管理、分段、能力探测
│   ├── src/recovery*.rs     #   断流恢复状态机
│   ├── src/danmaku/         #   弹幕 WS 协议、签名、XML 写入、ASS 转换
│   ├── src/database.rs      #   SQLite 持久化与迁移
│   ├── src/service.rs       #   业务操作层（桌面/HTTP 共用）
│   ├── src/bus.rs           #   事件总线（桌面 IPC / 服务端 SSE）
│   └── proto/               #   webcast protobuf 定义
├── src-tauri/               # Tauri 桌面壳（窗口、托盘、命令）
├── server/                  # 无头 Web 服务（axum + SSE + 内嵌前端）
├── src/                     # Vue 3 前端
├── deploy/                  # systemd unit
├── docs/FORK.md             # 作为 fork 分发的清单
└── .github/workflows/       # CI / Release / gitleaks
```

### 常用命令

```bash
pnpm install
pnpm dev                       # 前端调试
pnpm build                     # 构建前端（被服务端内嵌）
pnpm test                      # 前端测试
pnpm tauri dev                 # 桌面开发
npx tauri build                # 桌面打包（deb/rpm/AppImage 或 NSIS/MSI）

cargo test --workspace         # 后端全部测试
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p douyin-recorder-server   # 无头服务端
scripts/scan-secrets.sh        # gitleaks 扫描（需本地安装）
```

### FFmpeg sidecar（仅桌面打包需要）

打包时 Tauri 会查找按目标三元组命名的二进制：

| 平台 | 文件名 |
|------|--------|
| Linux x86_64 | `src-tauri/binaries/ffmpeg-x86_64-unknown-linux-gnu` |
| Windows x86_64 | `src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe` |
| macOS Intel | `src-tauri/binaries/ffmpeg-x86_64-apple-darwin` |
| macOS Apple Silicon | `src-tauri/binaries/ffmpeg-aarch64-apple-darwin` |

推荐使用 [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases) 的静态构建。
CI 的发布工作流会自动下载并放置。

---

## 更多功能细节

监控模式（限时/持续）、停止语义、托盘行为、断流恢复全过程、诊断日志（含接口响应抓取）、
分段录制细节、画质档位实测、旧版设置兼容等详细说明，见 [docs/USAGE.md](docs/USAGE.md)。

## 常见问题

**Q：弹幕文件为空 / 没有弹幕？**
A：XML 在收到第一条弹幕时才创建；人气低的直播间可能整场只有几条。若长时间为 0，请查看日志中的
`danmaku_start_failed` / `reconnecting`。

**Q：烧录字幕报错 "No such filter: ass"？**
A：当前 FFmpeg 未启用 libass。可先「导出 ASS」用播放器加载，或使用官方 Docker 镜像（已内置）。

**Q：容器里的文件属主是 root？**
A：请设置 `.env` 中的 `PUID`/`PGID` 与宿主机用户一致，并 `sudo chown -R $(id -u):$(id -g) data`。

**Q：录制历史会无限增长吗？**
A：历史记录本身没有上限，但列表按 **200 条/页** 加载，可点「加载更多历史」继续翻页，
因此几千上万条记录也不会拖慢页面。清理旧记录请在任务列表手动删除（默认保留文件）。

**Q：rpm 打包很慢？**
A：正常现象。rpm 打包器在进程内用 gzip 压缩约 175 MB 的 FFmpeg，通常需要 7~8 分钟。

---

## 致谢与许可

- 上游项目：[SplashedWhite/douyin-recorder](https://github.com/SplashedWhite/douyin-recorder)（MIT）
- 弹幕协议与算法参考：
  [rust-srec](https://github.com/hua0512/rust-srec)（Apache-2.0）、
  [DouyinLiveWebFetcher](https://github.com/saermart/DouyinLiveWebFetcher)（MIT）、
  [DouyinLiveRecorder](https://github.com/LyzenX/DouyinLiveRecorder)、
  [danmaku2ass](https://github.com/m13253/danmaku2ass)、
  [danmu2ass](https://github.com/gwy15/danmu2ass)
- 录制引擎：[FFmpeg](https://ffmpeg.org/)（LGPL/GPL，作为独立进程调用）

本项目采用 MIT 许可证，详见 [LICENSE](LICENSE)；第三方组件见 [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)。
