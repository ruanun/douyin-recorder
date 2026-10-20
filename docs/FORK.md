# 作为 fork 发布

本项目基于 [SplashedWhite/douyin-recorder](https://github.com/SplashedWhite/douyin-recorder)（MIT），
在其基础上增加了**弹幕录制**、**弹幕分段对齐**、**ASS 字幕导出/烧录** 与 **无头 Web 服务端**。

下面是把它作为独立 fork 分发时的检查清单。

## 1. 创建并关联 fork

```bash
# 在 GitHub 上 Fork 原仓库后：
git remote rename origin upstream
git remote add origin git@github.com:<你的账号>/<仓库名>.git
git push -u origin main
```

保留 `upstream` 便于后续同步上游：

```bash
git fetch upstream
git merge upstream/main        # 或 git rebase upstream/main
```

## 2. 仓库设置

| 位置 | 建议 |
|------|------|
| About | 描述写成 fork 定位（含 Web/无头服务端与弹幕字幕） |
| Actions | 默认开启；`release` 工作流需要 `contents: write`（已在工作流中声明） |
| Packages | 推送到 GHCR 需要允许 Actions 写包（默认允许） |
| Secrets | 如需自定义，添加 `GITHUB_TOKEN` 已自动可用，无需额外配置 |
| Branch protection | 对 `main` 开启：要求 CI 通过后再合并 |

## 3. 版本号

发布新版本时需同时修改三处（保持同步）：

- `Cargo.toml` → `[workspace.package] version`
- `package.json` → `version`
- `src-tauri/tauri.conf.json` → `version`

然后打标签触发发布工作流：

```bash
git commit -am "chore: bump version to 0.2.0"
git tag v0.2.0
git push origin main --tags
```

发布产物：

- Linux 桌面：`.deb` / `.rpm` / `.AppImage`（自动下载 FFmpeg sidecar）
- Windows 桌面：NSIS `.exe` / `.msi`
- 服务端：`douyin-recorder-server-<version>-linux-x86_64.tar.gz` + sha256
- 容器镜像：`ghcr.io/<owner>/<repo>:latest` 与 `:<version>`

## 4. 徽章（可选）

把下面的 `<owner>/<repo>` 换成分发仓库后放进 README：

```markdown
[![CI](https://github.com/<owner>/<repo>/actions/workflows/ci.yml/badge.svg)](https://github.com/<owner>/<repo>/actions/workflows/ci.yml)
[![Release](https://github.com/<owner>/<repo>/actions/workflows/release.yml/badge.svg)](https://github.com/<owner>/<repo>/actions/workflows/release.yml)
```

## 5. 许可证与致谢（必须）

- `LICENSE` 保留上游版权行；可在其后追加一行你自己的版权（同一 MIT 许可证）。
- 上游项目：<https://github.com/SplashedWhite/douyin-recorder>
- 弹幕协议/算法参考（已在 `THIRD_PARTY_LICENSES.md` 声明）：
  - <https://github.com/hua0512/rust-srec> (Apache-2.0)
  - <https://github.com/saermart/DouyinLiveWebFetcher> (MIT)
  - <https://github.com/LyzenX/DouyinLiveRecorder> (GPL-3.0，仅参考 XML 格式约定)
  - <https://github.com/m13253/danmaku2ass>（ASS 转换算法参考）

## 6. 发布前自检

```bash
# 依赖与构建
pnpm install --frozen-lockfile && pnpm build && pnpm test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 密钥扫描（CI 也会跑）
scripts/scan-secrets.sh

# 容器
docker build -t douyin-recorder-server:local .
```

确认以下文件**没有**被提交：

```bash
git ls-files | grep -E '^(data/|dist/|target/|\.env$|src-tauri/binaries/)' || echo "clean"
```
