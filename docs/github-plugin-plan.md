# GitHub 插件初步方案

2026 年 9 月 30 日。GitHub 插件通过动态库和固定组件驱动验证插件框架，使用用户已登录的 gh，不单独保存 GitHub token。第一轮交付包含 PR 浏览、创建、AI 审核草稿、人工审核发布和合并。

## 仓库与页面

侧栏提供 GitHub 入口和打开 PR 的数量。仓库通过 remote 推导 host/owner/repo，允许在表单中明确修改目标，所有 gh 调用显式传递 --repo。支持 origin 与 upstream 选择；fork 创建时区分 head 仓库和 base 仓库，跨仓库 head 应填写 owner:branch。企业 host 可手工绑定，认证交给 gh。列表展示编号、标题、作者、base/head、草稿和更新时间；状态筛选支持 open、closed、merged、all，并允许搜索。

详情显示正文、作者、提交 SHA、检查、审核状态和只读 diff。v1 使用宿主 Code 驱动，后续迁移到现有双栏差异驱动。列表分页提供上一页和下一页，不把某页数量当成打开 PR 总数；徽标通过独立总数查询更新。

## 命令与交互

| 功能 | gh 能力 | 第一轮行为 |
| --- | --- | --- |
| 环境检查 | gh --version、auth status | 工作线程检测，提示安装或 gh auth login |
| 列表及详情 | pr list/view、api | 明确 repo，分页，结构化 JSON |
| 创建 PR | pr create | 标题、正文、base、head、draft，预览确认，要求分支已经推送 |
| 审核 | api pulls reviews | Comment、Approve、Request changes，人工编辑并确认 |
| 合并 | pr merge | merge、squash、rebase，fresh head 校验和 --match-head-commit |
| 关闭和重开 | pr close/reopen | 显示操作对象并确认，不删除分支 |
| fetch 事件 | 宿主通知 | 只刷新打开 PR 数量，当前列表按需刷新 |

所有耗时操作异步执行，点击立即显示目标与 busy。远端写入先返回确认摘要和捕获的目标/参数，宿主确认后发起第二次请求；期间表单冻结，目标不能被切换。确认时重新检查 PR head，过期草稿要求重新审核。禁止 --admin，禁止合并时 --delete-branch，禁止在审核时隐式 checkout。

命令按参数数组调用，正文通过 stdin，不拼 shell。子进程隐藏窗口，关闭交互输入，设超时、输出限制并支持取消。只读失败可重试，远端写入结果不明时不自动重试，提示先刷新检查结果。合并队列或自动合并不冒充已合并；成功后读取 PR 最终状态。

## AI 审核

使用宿主已配置的模型，插件只提交审查材料，不获取 API key。冻结 head/base SHA，读取 PR diff，收集可复核的问题，返回结构化严重程度、文件、行号、描述和建议。结果生成本地草稿，通过 TextArea 编辑后手工发布；不自动批准或合并。正文、代码和评论是非可信审查材料，不能作为工具或系统指令。

第一轮为有明确覆盖限制的 diff 审核：过大 diff 明确拒绝并要求缩小范围，不伪称全仓库审查。二进制、缺少上下文和不能证明的问题须写明。后续通过只读 Git 服务读取 immutable tree 的关联代码，加入逐条选用、行内评论、持续本地草稿和审核历史。

## 后续范围

worktree 检出、行内审核线程、标签和审核人编辑、自动合并、CI 日志与重跑、Issues 和 Releases 均在通用接口验证完成后加入。本轮不做自动监控或自动发布。

## 验证

用假 gh 验证参数、正文 stdin、分页、失败和 SHA 变化，无需真实写入 GitHub。通过宿主实际加载 DLL，读取当前仓库的真实 PR 列表和总数。真实发布审核、创建和合并仅由用户在页面确认，不用测试修改公开 PR。AI 网络路径在有模型配置时验证；没有配置时必须准确提示。

参考 [PR 命令](https://cli.github.com/manual/gh_pr)、[审核](https://cli.github.com/manual/gh_pr_review)、[合并](https://cli.github.com/manual/gh_pr_merge)。

## 第一轮实现与验证

PR 列表、状态筛选、分页、详情、检查状态、diff、创建预览、审核预览、合并、关闭和重开均已接入 gh。AI 使用宿主服务，保留结构化报告，并生成可编辑的 Markdown 审核草稿。未绑定的其他托管平台不会自动收到 GitHub fetch 查询；切换目标仓库会清除旧审核草稿。

10 项插件测试通过，包括确认期间 SHA 更新阻止合并、人工审核固定 commit_id、正文 stdin、AI 草稿不自动发布、分页与独立 PR 总数、仓库切换和非 GitHub remote。真实 DLL 查询 adoin/git-Agent 得到 0 个 PR；查询 cli/cli 得到第一页 30 项和打开总数 62，读取 PR 14485 的 6055 字节 diff。这些数量是验证时快照，后续可能变化。

真实创建、发布和合并未用于测试，写入流程使用假 gh 验证。AI 网络服务未进行实际模型调用，真实鉴权与模型结果仍需在已配置模型的环境中验收。v1 暂无行内评论、逐条采纳、worktree 检出和持久化审核历史。

## 多页面与同步

工具栏提供带功能 SVG 的 PRs 待合并数量、Issues 和 Actions 按钮；省略号菜单还包含 Releases、仓库概览和设置。品牌图标已移除，文字与尺寸复用宿主工具栏。所有页面替换现有右侧主内容区，左侧 Git 导航保持原用途。PR 列表、详情、创建和审核分别是独立页面。Issues 支持列表、详情、创建、编辑、评论、关闭和重开；Actions 支持运行与工作流列表、详情、失败日志、重跑、取消和手动触发；Releases 支持列表、详情和浏览器打开；仓库概览读取 gh repo view。相关入口同时注册到顶部查看、仓库、操作和工具菜单。

待合并数量统计打开的非草稿 PR，草稿和打开总数另行显示；这不代表所有待合并 PR 都已通过 CI 或没有冲突。通过 gh api graphql 分页读取 isDraft，避免把列表某页数量或搜索索引延迟当成总数。查询失败保留上次成功数量和错误提示，首次查询前显示省略号。

切换仓库、后台定时同步、获取、拉取和推送完成后，以及创建、合并、关闭、重开和草稿状态变化后更新数量。插件页可见时会更新对应列表；创建或编辑草稿页面不因定时同步重新初始化输入。定时同步遵循应用现有后台远端刷新设置。

Actions 重跑、取消和工作流触发都先显示目标与参数再确认；重跑确认前核对运行 ID、head SHA 和 attempt。真实写入测试只创建下面明确授权的测试 PR，其他 Issue、Actions 写入使用模拟 gh 验证。参考 [gh run](https://cli.github.com/manual/gh_run) 和 [gh workflow run](https://cli.github.com/manual/gh_workflow_run)。

## 测试仓库样例

在独立克隆中创建以下 PR，没有修改 test-repo 原工作区的冲突现场，也没有推送 main。

| PR | 类型 | 验证用途 |
| --- | --- | --- |
| [1](https://github.com/adoin/test-repo/pull/1) | 普通待合并 | 列表、diff、审核、合并、关闭和重开 |
| [2](https://github.com/adoin/test-repo/pull/2) | 草稿 | Draft 展示及待合并计数排除草稿 |
| [3](https://github.com/adoin/test-repo/pull/3) | 故意包含缺陷 | 百分比未除以 100，供 AI 审核和 Request changes 测试 |
| [4](https://github.com/adoin/test-repo/pull/4) | 冲突 | GitHub 已显示 CONFLICTING，目标为专用 conflict-target 分支 |

验证时该仓库打开总数为 4，待合并为 3，草稿为 1。数量会随后续测试变化。

## 第二轮验证结果

689 项库和脚本测试通过，另有 1 项真实 DLL 测试通过，4 项原有测试保持忽略。真实加载验证了 12 个独立页面的布局，查询 test-repo 的 PR 列表、数量、冲突 diff，以及 Issues、Actions、Releases 和仓库概览。没有工作流时 gh 的空 stdout 已作为空列表处理。

原生窗口控件树确认工具栏存在 PRs (3)、Issues 和 Actions，左侧保持 Git 仓库导航。截图捕获两次超时，点击缺少坐标几何信息，因此本轮没有完成像素级布局验收；页面归属通过主内容区回归测试和真实 DLL 布局验证。AI 在线调用及其他远端写操作没有实际执行。

## 仓库标签星标与设置页调整

保留 PR 上一页和下一页分页。GitHub 设置页、连接设置链接和对应工具栏/顶部菜单入口已移除，插件使用当前 Git remote 自动识别 GitHub 仓库，优先 origin，再选择其他 GitHub remote。GitHub Enterprise 可通过 gh 的 GH_HOST 配置识别。

仓库 Star 数与 PR 数量在同一次异步 GraphQL 查询中读取。插件通过宿主通用的 repository_tab_badge 贡献显示仓库标签附加信息，图标和缩写数量位于仓库名与关闭按钮之间；Star 为 0 或尚未查询时隐藏，1,234 显示为 1.2k，悬浮显示精确数量。所有图标资源在加载阶段缓存，标签绘制不执行网络或文件查询。仓库 remote 改变时隐藏旧仓库的数量，等待新数据。

GitHub 的 PR 与 Star 同步现已订阅 repository.remote_sync.started，在 Git 开始获取、拉取、推送时并行启动；后台 fetch 同样适用。列表保持当前筛选与分页，state_patch 只更新远端数据，保留人工编辑的审核草稿和确认对象。

## GitHub 插件独立维护

GitHub 业务源码已移至 git-agent-plugin-template 默认项目，最小示例保存在该仓库 examples/minimal。主程序工作区只保留 SDK，测试用的 manifest、图标和字典位于 tests/fixtures/plugin-contract，没有插件业务代码。

主程序 dev.ps1 可构建并监听同级独立插件仓库，或通过 -ExternalPluginProjects 指定路径。正式安装器按 bundled-plugins.lock.json 获取固定版本包，验证提交和 SHA256 后预装。先发布插件，再发布引用该版本的主程序；本轮迁移没有创建发布标签。
