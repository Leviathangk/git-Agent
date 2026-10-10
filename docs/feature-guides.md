# 功能指引扩展

功能指引由 src/tutorials.rs 的 GUIDES 按顺序注册。每段包含固定 id、revision、双语标题 key 和 steps；每步声明目标区域、标题 key 与正文 key。基础指引保持 basics，插件指引使用 plugins。

新增需要强调的功能时，添加步骤数组和一个 Guide，再在 src/i18n.rs 中提供对应的中英文文案。可复用现有 Target；需要定位新的界面时，增加 Target 并补充 app.rs 的区域定位和 prepare_tutorial_step 页面准备。帮助菜单从注册表生成，不需要另加菜单分支。修订号仅在需要已有用户重新体验该段时提升，普通文字修正不提升。

首次启动会按注册顺序排入未体验的段落。旧 beginner_tutorial_seen=true 迁移为基础指引修订 1 已体验，不会把插件段落一并标记。完成、跳过或按 Esc 只消费当前段落，随后进入下一段。退出时尚未结束的段落在下次启动重新开始。基础段落末尾的字体设置会暂停队列，关闭设置后继续。

进度保存到应用数据目录的 tutorial-progress.json，结构为 versions 中的 id 与修订号映射。跳过与完成使用相同的已体验记录。未知 ID 和更高修订号保留，避免降级或移除段落时丢失记录。该文件不经过 AI 凭据加密路径，因此系统密钥库错误不会阻止引导进度保存。

写入通过 Persistence 的具名任务、接收器和队列完成；界面先进入下一步或下一段，再提交最新快照。并发请求合并为最新快照，临时文件原子替换，退出时等待已有写入并刷新最终状态。失败时释放任务所有权并提示，下次可能再次展示未保存的段落。

引导期间使用前景遮罩和模态焦点层，并阻止全局快捷键。插件设置使用实际绘制区域定位；没有打开仓库时，仓库插件步骤展示全局列表并说明如何在打开仓库后设置。引导只准备界面，不安装插件、调用 AI 或执行 Git 写操作。

## English

Register feature guides in src/tutorials.rs with a stable ID, revision, localized title and step array. Reuse existing targets or add a target and its geometry/page preparation in app.rs. Add both languages in src/i18n.rs. The Help submenu is generated from the registry. Increase a revision only when existing users should see that guide again.

The queue presents unseen guides in order. Legacy basic-tour state applies only to basics. Completion, skipping and Escape consume the current guide and continue to the next; unfinished guides restart on the next launch. Font settings pause the queue until the settings window closes.

Progress is stored in tutorial-progress.json independently of AI secrets. Asynchronous, coalesced atomic writes retain receiver ownership, report failures and flush on exit. Unknown IDs and higher revisions survive downgrades. Foreground masking, modal focus and shortcut gating protect underlying actions; guide preparation only changes visible UI.
