# 动态库插件初步方案

2026 年 9 月 30 日。该方案用于把 Git Agent 的业务扩展拆成可独立开发、构建和安装的动态库，GitHub 插件是第一项验收用例。

## 已确定的接口方向

宿主负责原生界面、组件驱动、仓库上下文、异步任务、凭据和 AI 服务。插件负责业务逻辑，通过声明式布局描述多个独立页面，不链接 egui，不访问 GitAgentApp。页面使用现有右侧主内容区，保留顶部菜单、仓库标签、工具栏和左侧 Git 导航。动态库采用稳定 C ABI，Windows 使用 DLL，macOS 使用 dylib，Linux 使用 so。入口为 git_agent_plugin_entry_v1，函数表包含 ABI 版本、结构尺寸、invoke 和 release_buffer。v1 使用无状态请求，把插件状态显式放在请求和响应中；任务由宿主工作线程持有，不允许插件留下脱离生命周期的线程。

跨边界仅传固定结构和 UTF8 JSON 缓冲区。分配方释放自己的缓冲区，插件内部捕获 Rust panic，宿主验证协议和响应尺寸。原生崩溃无法隔离，能力声明不是操作系统沙箱；只安装信任的动态库。禁止运行中热卸载，更新通过重启应用生效。参见 [Rust FFI](https://doc.rust-lang.org/nomicon/ffi.html)。

## 布局与固定驱动

v1 基础驱动提供 Row、Column、Text、Input、Select、Button、Table、Form、Navigate 和 Code。Input 支持多行，Form 组合输入和提交按钮，Table 使用稳定行键。文本和选项支持中英文。Code 用于第一版只读 diff。后续加入宿主现有 DiffView 驱动、行内评论、Tabs、Grid 和 Markdown。

插件返回组件树和数据；输入绑定由宿主保存，事件携带 action、当前输入和上下文发送给插件。宿主在任务期间显示所选目标和进度，统一禁用同一页面的所有操作。纯渲染不调用动态库、不执行命令、不访问网络。组件树必须限制深度、节点数和大小。未知组件或未满足的驱动要求在加载时报告。

## 加载与开发目录

项目插件固定放在 plugins/<id>，manifest 为 plugin.json，SDK 在 crates/plugin-sdk。开发脚本构建动态库并把库和 manifest 复制到 target/dev-plugins/<id>，通过 GIT_AGENT_PLUGIN_DIR 告知宿主。宿主只从显式开发目录或程序旁 data/plugins/extensions 发现业务插件，不隐式执行正在打开的仓库里的代码。已有 data/plugins/syntax 保持原规则。

manifest 声明 id、name、version、api_version、各平台 library、drivers、capabilities、events。首版能力为网络、AI 和远端写入；这些声明由宿主检查服务请求，不提供隔离第三方本地代码的保证。重复 ID、越界库路径、不兼容版本、缺失入口和错误组件树必须可诊断，单个加载失败不影响其他插件。

## 任务与事件

库发现和插件 invoke 都在工作线程完成，receiver 在 spawn 前登记。状态按插件 ID 和本地仓库路径隔离，任务捕获分支、HEAD、remote 和语言，结果不能串到另一个仓库。取消任务通知宿主服务和子进程；协作取消完成前不释放 busy 门禁。库由 Arc 保持存活直至全部调用完成。

首版插件命令只读本地仓库、修改远端 PR，不允许 checkout 或写 index/worktree。后续本地写操作必须接入现有 RepositoryRefreshScheduler 排他门禁和 uncached reload。

fetch 完成通知先记录来源仓库，等待宿主新快照应用后发送 repository.fetch.completed；后台 fetch 也覆盖。事件仅在成功时触发；插件失败独立显示，不改变 fetch 结果。重复刷新合并，同一作用域繁忙时保留一个待刷新事件，30 秒内合并重复后台通知。事件不是拦截器，v1 不允许阻止 Git fetch。

## 实施与验收

1. SDK、manifest、动态库发现、ABI 和缓冲区协议。
2. 固定组件渲染、异步 receiver、作用域隔离、确认与取消。
3. 侧栏入口、fetch 事件、宿主 AI 服务。
4. 开发脚本自动构建、加载和监听 plugins 与 crates。

测试覆盖 ABI/驱动不兼容、路径越界、重复 ID、真实动态库加载、请求响应、组件渲染、即时 busy、断连和失败释放、取消、跨仓库结果隔离、fetch 刷新顺序、脚本构建顺序。实际运行 scripts/dev.ps1，验证应用进程及插件加载日志。

## 第一轮实现与验证

SDK、C 头文件、加载器、两个固定驱动、首轮侧栏入口、异步任务、确认和取消、fetch 通知及宿主 AI 服务已实现。运行 workspace 库测试和脚本测试得到 679 项通过、4 项原有测试忽略；另执行真实动态库测试，验证入口、布局、错误、取消和重复 ID，1 项通过。scripts/dev.ps1 完成自动构建、复制 DLL、启动应用及 8 秒运行检查，日志记录 github 插件加载成功。

当前布局与插件状态保存在应用会话中，已提供图形化安装管理和跨重启插件开关；跨重启草稿保存和本地写入 Git 服务仍未提供；这些属于下一轮接口扩展。加载与布局已经不依赖 GitHub 专有代码，后续业务插件可以复用 SDK 和驱动。

## 多页面与菜单贡献

第二轮把入口迁移到仓库工具栏：插件声明 SVG 图标、直接展示的页面按钮和图标菜单中的页面。顶部现有查看、仓库、操作、工具菜单按命令声明生成插件子菜单，保留原有标题栏拖动区域和布局。GitHub 不再占用左侧导航。

manifest.contributes.pages 定义页面 ID、双语标题、工具栏短标题、首次加载 action 和数量徽标；toolbar 声明 icon、pages 和 inline_pages；commands 声明 ID、双语标题、目标 page、action、menus。界面贡献只能引用已注册页面，未知菜单位置、重复 ID、越界 SVG 和不兼容驱动在加载时拒绝。

Request.page 标识当前页面，Response.navigate 声明结果要进入的页面。宿主按插件、仓库、页面缓存布局，业务 inputs/state 在同一仓库的插件页面间共享。用户切换页面立即改变内容区标题，旧任务只更新所属页面缓存，不把用户强制带回旧页面。远端写入确认保存完整请求，导航不能修改已冻结的操作目标。

新增 repository.activated、repository.sync、repository.pull.completed、repository.push.completed。显式获取、拉取和推送在 uncached reload 后刷新；定时同步沿用后台远端刷新开关、仓库级开关和间隔，对已打开且有快照的仓库独立查询，不因 Git/index 忙碌而漏掉只读插件同步。显式刷新不受 30 秒后台去重限制。

完整格式与示例见 [插件开发指南](plugin-development.md)。

## 独立模板与安装管理

独立模板仓库包含固定 SDK、双语页面、钩子、权限、联调与打包脚本。宿主提供安装、更新、卸载、全局及仓库开关，包校验和管理写入由有明确 receiver 的工作任务完成。更新及卸载通过注册表安排在下次启动生效，当前库仍由调用持有。详见 [安装指南](plugin-installation.md) 和 [开发指南](plugin-development.md)。

## GitHub 插件独立维护

GitHub 业务源码已移至 git-agent-plugin-template 默认项目，最小示例保存在该仓库 examples/minimal。主程序工作区只保留 SDK，测试用的 manifest、图标和字典位于 tests/fixtures/plugin-contract，没有插件业务代码。

主程序 dev.ps1 可构建并监听同级独立插件仓库，或通过 -ExternalPluginProjects 指定路径。正式安装器按 bundled-plugins.lock.json 获取固定版本包，验证提交和 SHA256 后预装。先发布插件，再发布引用该版本的主程序；本轮迁移没有创建发布标签。
