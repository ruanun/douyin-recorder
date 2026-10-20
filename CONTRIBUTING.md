# 贡献指南

感谢参与！本项目是 [SplashedWhite/douyin-recorder](https://github.com/SplashedWhite/douyin-recorder)
的 fork，新增了弹幕录制、ASS 字幕与无头 Web 服务端。

## 环境准备

```bash
# 依赖
node >= 18、pnpm 10、Rust stable、protoc、FFmpeg（桌面端打包需要 sidecar）

pnpm install
```

- 桌面开发：`pnpm tauri dev`（需要系统安装 FFmpeg，或把静态版放到 `src-tauri/binaries/`）
- 前端开发：`pnpm dev`
- 服务端本地运行：
  ```bash
  pnpm build
  DOUYIN_RECORDER_HOME=/tmp/dr DOUYIN_RECORDER_TOKEN=dev-token \
    cargo run -p douyin-recorder-server
  ```

## 提交前检查

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm build && pnpm test
scripts/scan-secrets.sh          # 需要本地安装 gitleaks
```

CI 会执行以上全部检查（见 `.github/workflows/ci.yml`）。

## 代码结构约定

- **业务逻辑只写 `core/`**：解析、录制、弹幕、数据库、字幕转换都在这里，禁止引入 Tauri/axum 依赖。
- **桌面命令**（`src-tauri/src/commands.rs`）与 **HTTP 分发**（`core/src/service.rs`）都只是薄壳，
  共享同一份业务函数；新增接口时两边都要登记。
- **事件**统一走 `core/src/bus.rs`：桌面转发到 webview，服务端转发为 SSE。
- 新增数据库字段必须写迁移（见 `core/src/database.rs` / `segment_store.rs` 的 `ALTER TABLE` 兼容写法）。

## 提交信息

建议使用 Conventional Commits 风格的前缀：

```
feat:     新功能
fix:      缺陷修复
refactor: 重构（不改变行为）
docs:     文档
test:     测试
chore:    构建/依赖/杂项
```

请让每个 PR 聚焦单一主题，并在描述中说明：背景、改动点、验证方式（命令或截图）。

## 新增平台/协议实现的注意事项

弹幕协议与签名算法有多个来源（见 `THIRD_PARTY_LICENSES.md`）。移植第三方代码时：

1. 在文件头注明来源与许可证；
2. 同步更新 `THIRD_PARTY_LICENSES.md`；
3. 提供可离线运行的单元测试（协议解析、时间轴、边界条件）。
