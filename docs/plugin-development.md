# 开发动态库插件

业务插件通过稳定 C ABI 运行，界面由 Git Agent 的固定驱动绘制。可参考独立仓库 https://github.com/adoin/git-agent-plugin-template 的默认项目，它不依赖 egui，只依赖 crates/plugin-sdk 和 serde_json。

## 目录与构建

独立开发优先使用 `D:/workspace/git-agent-plugin-template`（[模板仓库](https://github.com/adoin/git-agent-plugin-template)），其 README、打包脚本和固定 SDK 不依赖主项目目录。仓库内插件也可以在 plugins/<id> 创建 Cargo.toml、plugin.json 和 src/lib.rs，并在根 Cargo.toml 的 workspace.members 注册。Rust 库类型设置为 cdylib，依赖 git-agent-plugin-sdk；使用 export_plugin 宏导出处理函数。外部语言可使用 crates/plugin-sdk/include/git_agent_plugin.h。

运行 `pwsh.exe -NoLogo -NoProfile -File scripts/dev.ps1`。脚本自动发现固定目录中同时有 plugin.json 和 Cargo.toml 的插件，构建到 target/debug，再复制到 target/dev-plugins/<id>。应用只加载复制的动态库，不加载仓库源码目录。修改 plugins 或 crates 会先停应用再重建，避免 Windows DLL 被占用。

`-SmokeTestSeconds 5` 执行构建、加载、运行检查后结束，适用于自动验收。正常运行保持监听。构建失败保留日志，不启动旧插件。发布插件分别构建平台库并生成 `.gaplugin` 包，在“选项 → 插件”导入；安装、更新、卸载与开关见 [插件安装指南](plugin-installation.md)。手动开发目录 `data/plugins/extensions/<id>` 仍支持重启加载，托管版本优先于同 ID 手动版本，开发环境目录优先于托管版本。

## 请求与响应

请求包含 action、page、context、inputs、state、confirmed。context 有本地仓库路径、分支、可用的 HEAD、remote 和语言；历史尚未加载时 HEAD 可能为空，业务必须自行获取和冻结所需远端提交。inputs 是宿主维护的字符串绑定，state 是插件上次返回的 JSON 状态。

处理函数返回 Response，其中 ui 是 Node 组件树，state 是下一次请求所需状态，inputs 用于初始化或更新输入，badge 更新导航计数，notice 显示结果，error 显示失败。没有新的 ui 时沿用所属页面的原布局。Response.navigate 可进入另一个已注册页面，badge_tooltip 说明数量含义。组件 ID 在页面内唯一，table.row_key 应为稳定主键，table.on_select 事件将所选主键写入同名输入。

支持 row、column、text、input、select、button、table、form、code；具体 JSON 结构由 SDK 中带 serde tag 的 Node 定义。manifest.drivers 声明使用的 basic.v1、code.v1 或 diff.v1。Layout 最大深度 32、节点 2000、单表 500 行，请求响应最大 8 MiB。Code 预览在工作线程裁剪到 40000 字符，插件 state 保留完整内容。

返回 confirmation 时宿主冻结表单和请求，展示摘要。用户确认后，同一个 action 携带固定输入、状态及 confirmed=true 再调用一次。远端写入仍需插件重新核实目标和 SHA，confirmed 不应被当作跳过所有业务验证的标志。

## 服务与事件

Host.call 支持 cancelled 和 ai.review。ai.review 请求含 prompt，返回结构化审核报告字符串，凭据由宿主管理。调用服务需要声明 ai 能力。宿主在插件工作线程里处理回调；插件不能保留 HostApi 指针、借用的请求缓冲区或后台线程。每个库的调用串行，多个仓库的界面状态分别保存。

manifest.events 可订阅 repository.fetch.completed、repository.pull.completed、repository.push.completed、repository.activated 和 repository.sync，成功 Git fetch 和仓库快照刷新完成后收到同名 action。插件查询失败不能改变 Git fetch 的成功结果。回调里只做自己的业务刷新，不调用 fetch 形成递归。重复通知合并，用户动作繁忙期间保留一次待刷新事件。

取消通过 Host.cancelled 协作完成。插件执行外部命令必须限制时间、输出大小，并取消和回收子进程；取消前已开始的远端写入结果可能不明，应先刷新核实，不能自动重试。

## 验证入口

`cargo test --workspace --lib --test dev_script_tests` 验证框架、插件流程和脚本约定。运行开发脚本后，执行 `cargo test --test native_plugins_tests -- --ignored` 验证真实库的 ABI、布局、失败和取消。

`target/debug/git-agent-plugin-probe target/dev-plugins/github OWNER/REPO [PR_NUMBER]` 实际加载插件并调用 gh，只读取列表、数量和可选 PR diff，不发布或合并。

## 页面与菜单格式

每个页面独立绘制在现有右侧主内容区，不替换应用的顶部菜单、仓库标签、工具栏或左侧 Git 导航。业务插件通过以下贡献结构注册页面和命令：

```json
{
  "contributes": {
    "pages": [
      {
        "id": "prs",
        "title": {"zh": "Pull Requests", "en": "Pull Requests"},
        "toolbar_label": {"zh": "PRs", "en": "PRs"},
        "toolbar_icon": "assets/pull-request.svg",
        "entry_action": "refresh",
        "badge": true
      },
      {
        "id": "settings",
        "title": {"zh": "连接设置", "en": "Connection settings"},
        "entry_action": "open"
      }
    ],
    "toolbar": {
      "icon": "assets/more.svg",
      "pages": ["prs", "settings"],
      "inline_pages": ["prs"]
    },
    "commands": [
      {
        "id": "view_prs",
        "title": {"zh": "查看 PR", "en": "Pull requests"},
        "page": "prs",
        "action": "open",
        "menus": ["repository", "view"]
      }
    ]
  }
}
```

这个片段放入原 manifest，其他 id/version/api_version/library/drivers/capabilities 字段仍然必填。完整业务例子是独立仓库根目录 plugin.json。页面的 toolbar_icon 声明功能 SVG；toolbar.icon 声明更多功能菜单图标，可省略。SVG 存放在插件 assets 目录，开发脚本自动复制；加载时统一检查路径、大小和有效 SVG，并缓存所有页面图标，绘制不访问文件系统。建议使用单色白色 SVG，让宿主统一应用主题颜色。toolbar.pages 是图标菜单项目，inline_pages 是直接显示的页面按钮；badge=true 使用插件返回的数量。支持菜单位置为 repository、actions、tools、view，对应现有顶部菜单中的插件子菜单。

首次打开页面先调用 open 获得基础布局，再按 entry_action 加载数据；entry_action=open 用于纯配置或新建表单，不自动查询。Node.Navigate 声明页面切换按钮，格式为 type=navigate、label、page。请求的 page 固定本次布局归属，响应的 navigate 可指定新目标。后台结果只更新原页面缓存；用户已经切换页面时，不会被旧任务跳回。

同一插件仓库内的页面共享 inputs/state，因此字段名应按业务区分，例如 number、issue_number、run_id。任务和确认共用该仓库插件的 busy 门禁；页面导航仍可进入目标空间，后续调用等当前任务完成。确认请求保持原 page、仓库、参数和 SHA，不随导航变化。

数量和自动同步事件不应执行不受控写入。repository.sync 沿用宿主的后台远端刷新设置，显式获取和 Git 写操作完成后通知在新快照应用后发出。插件事件只读查询可独立于本地 index 的忙碌状态运行。

工具栏页面按钮和宿主 Git 按钮共用文字、图标、尺寸及悬停样式：11 号常规文字、13 像素图标、18 像素最小高度和相同内边距。插件只声明语义标签和功能图标，不分别选择字体或字号。GitHub 插件使用 PR 分支、Issue 提示、Actions 播放、Release 标签、仓库及设置图标，更多功能菜单使用省略号，未使用品牌图标。

## 表格列布局

Table 使用宿主的表格驱动，不使用嵌套 Grid 行布局。表头和每条数据共享列宽，固定行高，所有单元格保持单行；长文本截断后可悬浮查看完整内容，窄内容区横向滚动。点击任意单元格都会使用该行 row_key 触发 on_select。

Column 除 field 和 label 外，可声明 width、min_width、flex：

```json
[
  {"field": "number", "label": "编号", "width": 56},
  {"field": "title", "label": "标题", "flex": true, "min_width": 240},
  {"field": "kind", "label": "类型", "width": 72},
  {"field": "author", "label": "作者", "width": 112},
  {"field": "updated_at", "label": "更新时间", "width": 180}
]
```

width 是固定逻辑宽度，flex=true 使用剩余空间，min_width 限制最小宽度。旧插件省略这些字段时使用默认列宽。表格支持 1 至 32 列，列名不得重复，宽度须是 24 至 2000 的有限数值；固定宽度和 flex 不可同时设置。

## 仓库标签附加数量

插件可以在 contributes 中声明只读仓库标签徽标：

```json
{
  "repository_tab_badge": {
    "icon": "assets/star.svg",
    "label": {"zh": "Star 数", "en": "Stars"}
  }
}
```

异步响应的 repository_tab_count 返回完整非负整数，0 隐藏徽标，省略字段保留上一次有效数据。宿主显示图标及紧凑数量，例如 182、1.2k、1.2m；悬浮提示显示精确值。数据按插件和本地仓库隔离，并记录来源 remote，remote 改变后不展示旧数据。徽标只绘制和显示提示，不修改仓库，也不占用关闭按钮的点击范围。

为徽标预留的宽度参与仓库标签的溢出计算和实际绘制，名称、图标、数量及关闭按钮使用独立区域，标签点击、重命名与拖动沿用宿主交互。SVG 加载与校验在后台完成，绘制阶段仅使用缓存。Github 插件的连接设置页已经移除，此处前文的 settings 页面仅作为其他插件的贡献格式示例。

## 远端同步开始钩子

订阅 repository.remote_sync.started 后，手动获取、拉取、推送和后台 fetch 在启动 Git 工作线程前同时登记并异步唤起插件任务。请求的 event 包含 operation_id、operation（fetch、pull、push）和 source（manual、background）。钩子不是同步阻塞回调，不拦截或改写 Git 参数。

每个钩子有独立 receiver 和取消标志，与 Git 任务并行调度，不等待 Git 完成或仓库重载。动态库内调用仍按 ABI 约定串行；若同一个库正在执行其他动作，工作线程等待库的调用门禁，UI 不阻塞。

开始钩子用于只读远端同步，不能返回 confirmation 或 navigate，不能执行本地 checkout、写 index 或重复 fetch。使用 state_patch 返回要更新的顶层业务数据，例如 counts、stars、rows；宿主合并这些字段，保留 inputs、审核草稿和已冻结的确认请求。可同时返回数量徽标和当前列表布局。前台插件调用尚未完成时，结果保留在钩子任务中，待前台结果处理后应用。

工具栏同步状态和仓库忙碌门禁覆盖 Git 操作及全部参与钩子的生命周期。Git 完成但钩子尚未完成时，显示“正在同步远端插件”；钩子成功、失败或 receiver 断连后释放对应门禁。插件失败单独报告，不把成功的 Git fetch 改成失败。插件页面在标题旁显示 loading，钩子完成后消失；页面不提供单独的同步取消按钮。

原有 completed 事件仍供需要新本地快照的插件订阅。GitHub 已改为订阅开始钩子，在同一阶段读取 PR/Star 数量，并在 PR 页面刷新当前分页，避免再在完成事件中重复请求。

## 页面标题栏与操作区

Page 节点可将布局拆成 header 和 body。宿主仍绘制页面标题，header 中的组件在标题右侧展示，body 占用主内容区。旧插件使用 Column 作为根节点时沿用原布局。

IconText 使用已注册 SVG 展示图标和简短文本，tooltip 解释含义。额外的图标通过 contributes.icons 声明，统一在后台加载、校验并缓存。

ActionBar 提供 left、right 两组控件及 right_width，宽窗口将右组对齐到操作行右侧；窄窗口分行排列。Input 可声明 width 限定输入框宽度。PR 页面用标题右侧三个图标数字表示待合并、草稿和打开总数，编号查询与当前分支入口放在搜索行右侧；成功刷新不再显示常驻提示。

## 插件双语字典

插件通过 manifest.locales 声明自己的 UTF-8 JSON 字典。开发脚本按声明的相对路径复制字典，宿主加载时校验路径和内容并缓存；绘制阶段不读取文件。

```json
{
  "locales": {"zh": "locales/zh.json", "en": "locales/en.json"},
  "contributes": {
    "pages": [{"id": "prs", "title": {"key": "pages.prs"}}]
  }
}
```

字典使用相同的键，例如 zh.json 的 `{"pages.prs":"PR 列表","state.open":"打开"}` 和 en.json 的 `{"pages.prs":"Pull Requests","state.open":"Open"}`。页面、工具栏和菜单标题可用 `{"key":"pages.prs"}` 引用，原有 `{"zh":"中文","en":"English"}` 仍可使用。组件标签及 notice、confirmation 等文字通过 `{{i18n:state.open}}` 引用，也可嵌入带参数的提示文字。SDK 的 text_key 和 Dictionary 提供引用、解析和区域语言回退；缺少当前语言时回退英文，缺少字典键时显示键名。

Select 的 options 保存内部值，option_labels 提供显示映射，例如 `{"open":"{{i18n:state.open}}"}`。表格列的 value_labels 同样映射枚举和布尔值。宿主只翻译显示标签，保留表格原始行数据、输入绑定、命令参数及 Code 内容。用户填写的标题、正文和审核草稿保持原文。

宿主按页面和当前语言缓存翻译后的布局，切换语言时从原始组件树重新翻译，不发起远端查询；异步响应更新布局后清理翻译缓存。标题、工具栏、选项和已有列表无需等待下一次 fetch 才更新语言。单个字典最多 256 KiB 和 2000 个键。GitHub 的字典位于独立仓库 locales，PR 状态、草稿、议题和工作流标签均在插件内维护。

## 固定分页与列表视口

Viewport 节点将列表分为 top、content 和 bottom 三个区域。顶部放搜索与操作控件，底部固定分页器，content 使用剩余高度。content 为 Table 时，宿主固定表头并让数据行在表格内部滚动；页面外层不再包裹整页滚动。空列表、少量数据和 50 行数据使用相同的底部分页位置。其他页面可以继续使用原有 Column 布局。

Pagination 声明 page、page_count、total、page_size、page_sizes，以及 page_key、size_key、on_page、on_page_size。宿主将页码或每页条数写入对应 inputs 绑定，再异步发送动作。第一页禁用上一页，末页禁用下一页，页码按钮高亮当前页；页数多时显示当前页附近的页码和首末页。labels 提供 total、size、unit、previous、next、page 的字典引用；total 与 page 文案可使用 {total} 和 {page} 占位符。

GitHub PR 列表允许每页 10、20、30、50 条，默认 30 条。改变每页条数或筛选条件后回到第一页；远端同步保留当前每页条数。页数按每页条数重新计算，仍遵循 GitHub 搜索最多访问 1000 条结果的限制。远端删除 PR 导致当前页超出末页时，插件重新读取有效页面，再释放忙碌门禁。

## 统一列表页面与溢出菜单

PR、议题、运行记录和工作流定义使用相同的 Page、ActionBar、Viewport、Pagination 结构。列表页省略重复的仓库名称和常驻刷新说明，标题高度统一，加载状态只在标题旁显示。搜索、筛选与刷新放在顶部左侧，编号查询和子页面入口放在右侧；每页条数、表格内部滚动和固定底部分页沿用同一驱动。

Toolbar 的 pages 声明溢出菜单候选项，宿主自动排除 inline_pages 中已有的页面并去重，候选项为空时隐藏更多按钮。GitHub 工具栏直接提供 PR、议题和工作流，三点菜单只显示发布与仓库概览。顶部菜单可以继续提供这些功能的快捷入口。

运行记录页只显示运行列表；工作流定义与手动触发表单分别位于 workflows 和 workflow_dispatch 页面。点击工作流定义行进入触发页面，原有确认流程和远端参数验证继续生效。议题与运行记录使用 gh api 分页查询，每页允许 10、20、30、50 条。筛选或每页条数变化后回到第一页，远端数量减少时重新查询有效页面。

全局 repository.remote_sync.started 在 PR 页刷新当前 PR 分页，在议题、运行记录或工作流定义页刷新对应的当前分页，并同步 PR 与 Star 统计。资源列表使用 issue、run、workflow 各自的状态和输入绑定，互不覆盖分页设置；同步响应继续保留用户草稿与冻结的确认请求。

页面贡献可用 parent 声明所属页面，例如 workflow_dispatch 的 parent 为 workflows，workflows 的 parent 为 actions。宿主沿父页面链保持对应工具栏栏目选中；未声明 parent 的旧插件仍按原页面 ID 判断。父页面必须存在，循环关系会在插件加载时拒绝。

## 仓库远端域名条件

插件可在清单中声明 repository_match，例如 GitHub 使用 `{"repository_match":{"remote_hosts":["github.com"],"host_env":"GH_HOST"}}`。remote_hosts 支持精确域名和 `*.example.com` 子域名规则；子域名规则不包含根域名本身。host_env 可引用一个额外的主机名配置，宿主在插件加载时缓存该值。域名条件不是必填项：未声明 repository_match，或 remote_hosts 为空且未设置 host_env 时，在所有仓库生效。只有声明了有效匹配规则的插件，才在无远端或域名不匹配时不执行。

宿主根据远端 URL 的实际主机名判断，识别 HTTPS、SSH、git 协议及 SCP 风格地址，并忽略用户名和端口进行域名匹配。任一远端匹配即可启用插件。按钮、菜单、仓库徽标、页面入口、初始化、同步钩子及最终动态库调用使用相同条件；不会因为插件目录已安装，就在全部仓库中显示入口或发起请求。

仓库切换到不适用的项目时，主内容区恢复工作区。远端改变时取消旧任务，清除旧页面缓存、徽标和冻结确认，丢弃迟到结果，保留用户草稿；新的匹配仓库随后重新初始化。GitHub 默认匹配 github.com，以及 GH_HOST 指定的企业主机。

## 插件 store 与声明式权限

每个插件在每个仓库中都有独立的 JSON store，与表单 inputs 和业务 state 分开。Request.store 携带当前快照。插件可以通过 Response.store_patch 合并更新，Response.store 整体替换，或调用 Host.call 的 store.get 与 store.patch；回调的更新在任务结果到达后按顺序应用，不覆盖期间新增的其他字段。null 表示删除字段。store 的更新不会修改用户草稿或生成新的布局查询。

任意按钮或布局可以直接声明 permission：

```json
{
  "type": "row",
  "permission": "store.access.manage",
  "children": [{
    "type": "button",
    "label": "创建",
    "action": "create",
    "permission": "store.access.create"
  }]
}
```

宿主将平铺 permission 转为 Guard 节点，父布局和子控件的条件共同生效。只接受以 store. 开头的点路径，只把 JSON 布尔 true 当作允许；不存在、false 或其他值都隐藏，不执行 JavaScript 或表达式。未声明权限的旧控件保持原行为。SDK 提供 with_permission 生成同样的结构。

页面及菜单贡献也可声明 permission。manifest.action_permissions 为动作提供执行入口的条件，例如 `{"merge":"store.access.merge"}`；宿主同时检查当前页面、动作声明和已缓存布局中的 Guard，读取当前权威 store 后才派发请求，避免只隐藏按钮。确认后的执行同样检查当前 store。对远端写入，插件仍须在自己的处理函数中重新查询权限和验证目标。

GitHub 插件使用 access、account 保存查询得到的当前账号及仓库、PR、议题权限。未知或查询失败时清掉管理权限；同步和查看详情时更新，执行写操作前再次查询。主程序不理解 GitHub 的角色，只读取插件提供的布尔路径。store 当前保存在内存中，重启后重新建立；远端变更会清掉旧 store 和确认请求，用户草稿保留。Host store 接口单次快照上限 1 MiB。动态库属于可信插件，store 门禁不是恶意原生代码的系统级沙箱。

## 文件改动与状态徽标

DiffBrowser 使用 diff.v1，files 中声明 path、patch、additions、deletions、truncated，labels 声明 files、empty、unavailable、truncated。放在 Viewport.content 时文件列表和原生 diff 各自内部滚动。长路径与增删数量单行排列，省略前缀后保留文件名，悬浮显示完整路径；每个文件预览最多 100000 字符。

Disclosure 提供可折叠的 title 和 text，适合详情页正文和检查摘要。Table 列 value_badges 将内部值映射到 tone 与 icon，tone 支持 success、danger、warning、info、neutral，icon 支持 check、cross、clock、play、stop、skip、dot、attention；宿主绘制主题适配徽标。

## 安装管理与开关

“选项 → 插件”管理安装和全局开关，“仓库设置 → 插件”提供继承、启用、关闭覆盖。最终条件同时包含全局开关、仓库覆盖和域名条件，覆盖不会越过全局关闭。新装插件异步加载，更新与卸载在重启后生效。关闭移除入口并停止新派发，取消请求中的任务仍持有 receiver 与库直到任务收尾，晚结果不会恢复权限或旧布局。

安装包的校验在工作线程进行，不执行动态库；启用后由 ABI 加载器检查入口。安装记录和开关保存在应用数据目录 plugins/registry.json，版本包位于 plugins/packages/<id>/<version>。仓库键规范化路径，Windows 不区分大小写。inputs/state/权限 store 仍是会话数据。

## 外部命令依赖

manifest.required_tools 可声明依赖命令名，例如 ["gh"]。宿主在后台按 PATH 检查可用性，管理页显示缺失项；安装依赖后刷新列表重新检测。此检查不执行命令，插件仍需在实际调用时处理错误和认证状态。

## GitHub 插件独立维护

GitHub 业务源码已移至 git-agent-plugin-template 默认项目，最小示例保存在该仓库 examples/minimal。主程序工作区只保留 SDK，测试用的 manifest、图标和字典位于 tests/fixtures/plugin-contract，没有插件业务代码。

主程序 dev.ps1 可构建并监听同级独立插件仓库，或通过 -ExternalPluginProjects 指定路径。正式安装器按 bundled-plugins.lock.json 获取固定版本包，验证提交和 SHA256 后预装。先发布插件，再发布引用该版本的主程序；本轮迁移没有创建发布标签。
