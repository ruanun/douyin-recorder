# 开发辅助脚本

| 脚本 | 用途 |
|------|------|
| `scan-secrets.sh` | 用 gitleaks 扫描 git 历史 / 暂存区 / 工作区（需先安装 gitleaks） |
| `repro-settings-save.mjs` | 无头 Chrome 回归：打开设置 → 保存，验证不再卡死（需 `DOUYIN_RECORDER_TOKEN`） |
| `repro-danmaku-upload.mjs` | 无头 Chrome 回归：上传 XML → 转 ASS 全流程 |
| `check-web-buttons.mjs` | 无头 Chrome 检查：Web 模式不出现「打开文件/打开文件夹」按钮 |
| `check-history-paging.mjs` | 无头 Chrome 检查：历史列表分页（首屏 200 条 + 加载更多） |

```bash
# 用法示例
scripts/scan-secrets.sh
DOUYIN_RECORDER_TOKEN=xxx node scripts/repro-danmaku-upload.mjs http://127.0.0.1:8787
```
