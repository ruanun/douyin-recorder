# 第三方许可证声明

本项目使用了以下第三方组件，特此声明：

## FFmpeg

- **组件**：FFmpeg
- **用途**：直播流录制和视频格式转换
- **许可证**：LGPL 2.1+ 或 GPL 2.0+（取决于编译配置）
- **源代码获取**：https://ffmpeg.org/download.html
- **使用方式**：作为独立可执行文件（sidecar）打包，通过进程间调用使用

本项目以 sidecar 方式打包 FFmpeg 可执行文件，通过命令行调用实现录制功能。FFmpeg 作为独立进程运行，与本项目代码无直接链接关系。

### FFmpeg 许可证信息

FFmpeg 的许可证取决于其编译配置：

- **默认配置**：LGPL 2.1+
- **启用 GPL 编解码器**（如 x264、x265 等）：GPL 2.0+

如需了解 FFmpeg 的具体编译配置和许可证详情，请访问：
- https://ffmpeg.org/legal.html
- https://ffmpeg.org/doxygen/trunk/md_LICENSE.html

### 源代码提供

根据 GPL/LGPL 许可证要求，FFmpeg 源代码可从以下地址获取：
- 官方网站：https://ffmpeg.org/download.html
- Git 仓库：https://git.ffmpeg.org/ffmpeg.git

---

## 弹幕协议参考实现

弹幕录制功能参考了以下开源项目的协议实现与数据格式：

### rust-srec (hua0512/rust-srec)

- **用途**：抖音弹幕 WebSocket 协议、X-Bogus 签名算法（`generate_xbogus`）与 XML 弹幕格式的 Rust 实现参考；本项目的 `src-tauri/src/danmaku/signature.rs` 移植自其 `signature.rs`
- **许可证**：Apache-2.0
- **仓库**：https://github.com/hua0512/rust-srec

### DouyinLiveWebFetcher (saermart)

- **用途**：抖音直播弹幕协议（`douyin.proto` 消息定义）的原始参考；本项目的 `src-tauri/proto/douyin_webcast.proto` 为其中的最小子集，字段编号与原定义严格一致
- **许可证**：MIT
- **仓库**：https://github.com/saermart/DouyinLiveWebFetcher

### DouyinLiveRecorder (LyzenX)

- **用途**：B 站兼容 XML 弹幕文件格式（`<d p="..." user="...">` 节点约定）的参考
- **许可证**：GPL-3.0
- **仓库**：https://github.com/LyzenX/DouyinLiveRecorder

### danmaku2ass / danmu2ass

- **用途**：ASS 字幕生成算法参考（滚动 `\move` 轨道分配、时间轴与转义规则）
- **许可证**：MIT
- **仓库**：https://github.com/m13253/danmaku2ass 、 https://github.com/gwy15/danmu2ass

### X-Bogus 算法出处

X-Bogus 签名算法源自抖音 Web 端 `webmssdk.js`（前端脚本），上述项目均基于同一公开算法。本项目为纯 Rust 独立实现。

---

本项目（douyin-recorder）本身采用 MIT 许可证，详见 [LICENSE](LICENSE) 文件。
