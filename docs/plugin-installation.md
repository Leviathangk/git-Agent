# 插件安装与管理

从 Git Agent 2.0.0-beta.3 开始，“选项 → 插件”负责导入插件包、查看加载状态、更新、卸载和全局开关；“仓库设置 → 插件”负责当前仓库的覆盖设置。安装包内置 GitHub 插件 0.1.0，首次启动自动安装，仍可关闭或卸载；卸载后不会在后续启动时强制重装。GitHub 功能需要安装并登录 gh，并只在匹配 GitHub 远端的仓库中启用。

## 安装和更新

选择“安装 / 更新插件包”，导入开发者提供的 `.gaplugin` 或 ZIP。包根目录需要 plugin.json、package.json、本机平台和架构的动态库、manifest 引用的字典与 SVG。宿主检查 API、驱动、能力、版本、资源路径、平台架构、包大小和 ZIP 路径；安装校验不执行动态库。

新插件安装后异步加载，列表展示实际加载结果。更新同 ID 插件需要更高的语义版本，旧版仍供当前会话使用，新版在重启后启用。缺失依赖或 ABI 入口不兼容会显示加载错误，安装成功不等于 ABI 加载成功。

安装成功后可以删除下载目录中的原始 `.gaplugin` 或 ZIP 文件。程序已将内容解压保存到自己的 `plugins/packages/<id>/<version>/` 目录，运行时使用该目录中的文件；更新包等待重启生效时也不再依赖下载文件。

动态库使用当前用户的系统权限。manifest 能力声明和 store 权限控制宿主服务与业务入口，不提供原生代码沙箱。

## 开关和适用仓库

全局关闭立即隐藏工具栏、菜单、页面及徽标，停止新同步和动作。已运行任务收到取消标志，保留任务所有权直到结束；远端写入结果不确定时应刷新核实。

仓库设置有“继承全局”“在此仓库启用”“在此仓库关闭”。仓库启用不能越过全局关闭，域名匹配条件仍然生效。省略域名条件的插件可以适用于所有仓库。开关和覆盖跨重启保存，插件自身的会话 store 不会因此持久化。

## 卸载和存储

“卸载（重启生效）”先关闭插件，重启后删除宿主管理的所有版本文件。开关与仓库覆盖保留。开发目录和手动扩展目录的源文件由开发者维护，宿主不会删除这些源码。

使用“复制插件目录”得到实际路径，避免猜测不同平台的数据目录。其内部结构为：

```text
plugins/
  registry.json
  packages/<id>/<version>/
  extensions/<id>/
```

registry.json 保存安装版本、待生效更新/卸载和开关。GIT_AGENT_PLUGIN_DIR 指定的开发目录优先，托管安装其次，extensions 手动目录最后；同 ID 不会运行两份。开发目录也遵守用户开关。

## 命令行验收

主程序构建后可使用 git-agent-plugin-manager，--plugin-dir 指向上述 plugins 根目录：

```powershell
git-agent-plugin-manager --plugin-dir D:/lab/plugins install D:/packages/example.gaplugin
git-agent-plugin-manager --plugin-dir D:/lab/plugins list
git-agent-plugin-manager --plugin-dir D:/lab/plugins disable example
git-agent-plugin-manager --plugin-dir D:/lab/plugins enable example
git-agent-plugin-manager --plugin-dir D:/lab/plugins repository example D:/workspace/test-repo off
git-agent-plugin-manager --plugin-dir D:/lab/plugins repository example D:/workspace/test-repo inherit
git-agent-plugin-manager --plugin-dir D:/lab/plugins probe example D:/workspace/test-repo
git-agent-plugin-manager --plugin-dir D:/lab/plugins uninstall example
```

probe 仅验证 open 布局，遵守全局和仓库开关。apply-pending 模拟宿主重启的生效阶段，应只在相关宿主进程已退出时用于验收。命令行修改后，运行中的宿主需通过刷新列表重新读取设置。

同一插件 ID 在本次宿主会话中复用库实例和调用门禁；关闭后库可保持驻留，但停止新调用。重启才切换 DLL 版本或删除托管文件，避免旧任务与新门禁并发进入同一原生库。
