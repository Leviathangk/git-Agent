# Plugin installation and management

Starting with Git Agent 2.0.0-beta.3, Options → Plugins imports packages and manages versions, load status and global switches. Repository Settings → Plugins manages repository overrides. Installers bundle GitHub plugin 0.1.0 and install it on first launch. You can disable or uninstall it; subsequent launches do not force an uninstalled plugin back. GitHub features require an installed, authenticated gh and activate only for repositories with matching GitHub remotes.

Import a .gaplugin or ZIP with plugin.json, package.json, the native library and referenced resources at its root. Validation checks API, drivers, capabilities, semantic version, target architecture, resource/archive paths and size without loading native code. New packages load asynchronously; higher-version updates activate after restart. Actual ABI or dependency failures appear as load errors.

After installation succeeds, you can delete the original .gaplugin or ZIP from your downloads folder. Its contents are saved in the application's plugins/packages/ID/VERSION directory and loaded from there. An installed update awaiting restart also no longer depends on the downloaded file.

Global disable hides all contributions and stops new actions/hooks. Active work is cancelled cooperatively and retains receiver/library ownership until completion. Repository overrides inherit or enable/disable locally and never bypass global disable or domain conditions. Switches persist; the plugin permission store remains session scoped.

Uninstall disables immediately and deletes managed versions on restart, retaining preferences. Development/manual source directories are not removed. Native plugins run with the user's permissions; host-service declarations are not a native sandbox.

Copy the actual plugin directory from the settings page. registry.json stores versions and switches; packages/ID/VERSION stores managed files; extensions/ID supports manual directories. GIT_AGENT_PLUGIN_DIR takes priority, then managed packages, then manual directories. Only one copy of an ID is loaded.

The git-agent-plugin-manager CLI accepts --plugin-dir DIRECTORY with list, install PACKAGE, enable ID, disable ID, repository ID PATH inherit|on|off, probe ID [REPO], uninstall ID and apply-pending. Probe validates only the open layout and honors switches. Use apply-pending only with the host stopped. Refresh the running app's plugin list after external CLI changes.
