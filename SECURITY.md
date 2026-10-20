# 安全策略

## 支持范围

只维护 `main` 分支与最新发布版本。历史版本的问题请先升级到最新版再确认是否仍存在。

## 报告问题

发现安全问题时，请**不要**公开提交 issue，改用 GitHub 的
[私密漏洞报告](https://docs.github.com/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
或直接联系维护者。请附上：

- 影响版本 / 部署方式（桌面 or 服务端）
- 复现步骤与预期/实际结果
- 是否涉及凭据、任意文件读写、远程代码执行等

## 自托管（服务端）加固建议

服务端模式会长期运行并对外提供 HTTP 接口，部署时请注意：

1. **务必设置访问令牌**：`DOUYIN_RECORDER_TOKEN`（≥ 16 字节随机值，`openssl rand -hex 16`）。
   未设置时接口完全开放，程序启动会打印警告。
2. **不要直接暴露到公网**：默认监听 `0.0.0.0:8787` 且为明文 HTTP。
   建议绑定内网地址，或置于反向代理（Nginx/Caddy）之后并启用 HTTPS：
   ```nginx
   location / {
     proxy_pass http://127.0.0.1:8787;
     proxy_set_header X-Auth-Token "你的令牌";   # 或让用户自行携带
     proxy_buffering off;                        # SSE 必须关闭缓冲
     proxy_read_timeout 3600s;
   }
   ```
3. **不要用 root 运行**：镜像默认以 UID 1000 运行；宿主机换用户时通过
   `PUID`/`PGID` 与 `chown` 保持一致，避免录像文件属主混乱。
4. **令牌存放在 `.env`**（权限 600）并加入 `.gitignore`；`gitleaks` 会在 CI 中扫描提交历史。
5. **数据目录**（`data/`，含数据库、日志与录像）应限制为服务运行用户可读，
   并纳入你自己的备份策略。Web 模式不提供浏览器下载接口，取用文件请走文件系统/共享。
6. 服务端只有单一共享令牌，没有多用户与权限模型——适合个人/小范围内网使用。

## 依赖与供应链

- CI 会运行 `cargo clippy -D warnings`、全部单元测试与 `gitleaks` 密钥扫描。
- 前端与 Rust 依赖使用锁定文件（`pnpm-lock.yaml`、`Cargo.lock`）。
- 建议开启 Dependabot / `cargo audit` 定期检查依赖漏洞。
