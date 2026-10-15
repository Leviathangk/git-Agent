---
status: prohibited
kind: prohibited-approach
recorded_at: 2026-10-13
scope:
  - release
reopen_only_if: 用户明确要求重新考虑此发布规则
---

# 禁止直接用正式标签试构建

禁止在目标版本的预发布未经完整验证时，直接推送正式标签触发首次发布构建。

证据：`v2.0.1` 的三平台刷新回归测试失败，没有发布安装包，却已占用正式标签；用户明确要求后续先发布 alpha 或 beta。

替代流程：遵循 [预发布验证后晋升正式版](../active/release-promotion.md)，失败时递增预发布序号，验证成功后创建新的正式标签，保留公开预发布标签。
